use crate::model::{self, UserToken};
use actix::{Actor, ActorContext, AsyncContext, StreamHandler};
use actix_web::{
    get,
    http::{KeepAlive, StatusCode},
    post, put,
    rt::{spawn, time::sleep},
    web::{self, ServiceConfig},
    App, FromRequest, HttpRequest, HttpResponse, HttpServer, Responder,
};
use actix_web_actors::ws;
use actix_web_httpauth::extractors::bearer::BearerAuth;
use anyhow::Context;
use futures::{
    channel::mpsc,
    future::{
        select,
        Either::{Left, Right},
    },
    Future, FutureExt, StreamExt,
};
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::{net::ToSocketAddrs, path::Path, pin::Pin, sync::Arc, time::Duration};

// Authorization is done using bearer tokens
impl FromRequest for UserToken {
    type Error = <BearerAuth as FromRequest>::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self, Self::Error>>>>;
    fn from_request(req: &HttpRequest, payload: &mut actix_web::dev::Payload) -> Self::Future {
        let auth = BearerAuth::from_request(req, payload);
        async move {
            let auth = auth.await?;
            Ok(auth.token().to_owned().into())
        }
        .boxed_local()
    }
}

fn respond<T: Serialize>(result: Result<T, model::Error>) -> HttpResponse {
    #[derive(Serialize)]
    struct ErrorPayload {
        error: model::Error,
    }
    match result {
        Ok(result) => HttpResponse::Ok().json(result),
        Err(error) => HttpResponse::build(match error {
            model::Error::UserNotFound => StatusCode::UNAUTHORIZED,
            model::Error::UserBusy => StatusCode::FORBIDDEN,
            model::Error::PipeNotFound => StatusCode::NOT_FOUND,
            model::Error::NotEnoughScore => StatusCode::UNPROCESSABLE_ENTITY,
            model::Error::ModifierAlreadyApplied => StatusCode::UNPROCESSABLE_ENTITY,
        })
        .json(ErrorPayload { error }),
    }
}

#[put("/api/pipe/{n}")]
async fn collect(
    state: web::Data<model::App>,
    user: UserToken,
    path: web::Path<usize>,
) -> impl Responder {
    let pipe_id = path.into_inner();
    respond(state.collect(&user, pipe_id).await)
}

#[get("/api/pipe/{n}/value")]
async fn pipe_value(
    state: web::Data<model::App>,
    user: UserToken,
    path: web::Path<usize>,
) -> impl Responder {
    let pipe_id = path.into_inner();
    respond(state.pipe_value(&user, pipe_id).await)
}

#[derive(Serialize, Deserialize)]
struct ApplyModifierInput {
    #[serde(rename = "type")]
    modifier: model::Modifier,
}

#[post("/api/pipe/{n}/modifier")]
async fn apply_modifier(
    state: web::Data<model::App>,
    user: UserToken,
    path: web::Path<usize>,
    input: web::Json<ApplyModifierInput>,
) -> impl Responder {
    let pipe_id = path.into_inner();
    let input = input.into_inner();
    respond(state.apply_modifier(&user, pipe_id, input.modifier).await)
}

#[get("/logs")]
async fn logs(
    state: web::Data<model::App>,
    req: HttpRequest,
    stream: web::Payload,
) -> actix_web::Result<HttpResponse> {
    struct LogsWs {
        state: web::Data<model::App>,
        sender: Option<mpsc::UnboundedSender<model::LogEntry>>,
    }
    impl Actor for LogsWs {
        type Context = ws::WebsocketContext<Self>;
        fn started(&mut self, ctx: &mut Self::Context) {
            let addr = ctx.address();
            let state = self.state.clone();
            let (sender, receiver) = mpsc::unbounded::<model::LogEntry>();
            self.sender = Some(sender.clone());
            spawn(async move {
                state.register_logs(sender.clone()).await;
                let mut receiver = receiver.boxed_local();
                while let Some(entry) = receiver.next().await {
                    addr.do_send(entry);
                }
            });
        }
        fn stopped(&mut self, _ctx: &mut Self::Context) {
            if let Some(sender) = self.sender.clone() {
                let state = self.state.clone();
                spawn(async move {
                    state.unregister_logs(&sender).await;
                });
            }
        }
    }
    impl actix::Message for model::LogEntry {
        type Result = ();
    }
    impl actix::Handler<model::LogEntry> for LogsWs {
        type Result = ();
        fn handle(&mut self, msg: model::LogEntry, ctx: &mut Self::Context) {
            ctx.text(serde_json::to_string_pretty(&msg).expect("Failed to serialize log message"));
        }
    }
    impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for LogsWs {
        fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
            let msg = match msg {
                Err(_) => {
                    ctx.stop();
                    return;
                }
                Ok(msg) => msg,
            };
            debug!("WEBSOCKET MESSAGE: {msg:?}");
            match msg {
                ws::Message::Ping(msg) => {
                    ctx.pong(&msg);
                }
                ws::Message::Pong(_) => {}
                ws::Message::Text(_) => error!("Unexpected text message"),
                ws::Message::Binary(_) => error!("Unexpected binary"),
                ws::Message::Close(reason) => {
                    ctx.close(reason);
                    ctx.stop();
                }
                ws::Message::Continuation(_) => {
                    ctx.stop();
                }
                ws::Message::Nop => (),
            }
        }
    }
    ws::start(
        LogsWs {
            state,
            sender: None,
        },
        &req,
        stream,
    )
}

fn configure(config: &mut ServiceConfig, state: web::Data<model::App>) {
    config
        .app_data(state)
        .service(pipe_value)
        .service(collect)
        .service(apply_modifier);
}

pub async fn run(
    addr: impl ToSocketAddrs,
    state: model::App,
    time_to_run: Option<Duration>,
    serve_dir: Option<impl AsRef<Path>>,
    enable_logs_api: bool,
) -> anyhow::Result<Arc<model::App>> {
    let serve_dir = serve_dir.map(|s| s.as_ref().to_owned());
    let state = web::Data::new(state);
    let server = HttpServer::new({
        let state = state.clone();
        move || {
            let mut app = App::new().configure(|config| configure(config, state.clone()));
            if enable_logs_api {
                app = app.service(logs);
            }
            if let Some(dir) = &serve_dir {
                app = app.service(actix_files::Files::new("/", dir).index_file("index.html"));
            }
            app
        }
    })
    .keep_alive(KeepAlive::Disabled)
    .bind(addr)
    .context("Failed to bind server")?
    .run();
    let server_handle = server.handle();
    let server_future = spawn(server);
    match time_to_run {
        Some(time) => match select(server_future, sleep(time).boxed()).await {
            Left((server, _sleep)) => {
                warn!("Server was shutdown before timeout was reached");
                server??;
            }
            Right((_sleep, server_future)) => {
                info!("Time is up, shutting down the server");
                server_handle.stop(true).await;
                server_future.await??;
            }
        },
        None => {
            info!("You can press Ctrl-C to stop the server");
            server_future.await??;
        }
    };
    info!("Server stopped");

    Ok(state.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{http::header::AUTHORIZATION, rt::task::spawn_blocking, test};
    use actix_web_httpauth::headers::authorization::Bearer;

    #[actix_web::test]
    #[ignore]
    async fn test_java() {
        crate::logger::init_for_tests();
        spawn_blocking(|| {
            let compilation_status = std::process::Command::new("javac")
                .arg("java_test/Main.java")
                .status()
                .unwrap();
            assert!(compilation_status.success());
        })
        .await
        .unwrap();
        let config = model::Config::default();
        let app = run(
            "127.0.0.1:8080",
            model::App::init(config, vec![]),
            Some(Duration::from_secs(2)),
            None::<&str>,
            false,
        );
        let client = async {
            sleep(Duration::from_secs(1)).await; // Wait for server to start
            let status = spawn_blocking(|| {
                std::process::Command::new("java")
                    .arg("java_test/Main")
                    .status()
                    .unwrap()
            })
            .await
            .unwrap();
            assert!(status.success());
        };
        let (app, ()) = futures::join!(app, client);
        app.expect("App error");
    }

    #[actix_web::test]
    async fn test_run() {
        crate::logger::init_for_tests();
        for _ in 0..100 {
            let config = model::Config {
                min_delay_secs: 0.0,
                max_delay_secs: 0.0,
                pipe_value_delay_secs: 0.0,
                min_value: 100,
                max_value: 200,
                ..Default::default()
            };
            run(
                "127.0.0.1:1234",
                model::App::init(config, vec![]),
                Some(Duration::ZERO),
                None::<&str>,
                false,
            )
            .await
            .unwrap();
        }
    }

    #[actix_web::test]
    async fn test() {
        crate::logger::init_for_tests();
        let state = web::Data::new(model::App::init(
            model::Config {
                min_delay_secs: 0.0,
                max_delay_secs: 0.0,
                pipe_value_delay_secs: 0.0,
                min_value: 100,
                max_value: 200,
                ..Default::default()
            },
            vec![],
        ));
        let app =
            test::init_service(App::new().configure(move |config| configure(config, state))).await;

        let req = test::TestRequest::get().uri("/").to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        let req = test::TestRequest::get()
            .uri("/api/pipe/1/value")
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        let auth = (AUTHORIZATION, Bearer::new("hello"));

        let req = test::TestRequest::get()
            .uri("/api/pipe/1/value")
            .append_header(auth.clone())
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let _: model::PipeValueResponse = test::read_body_json(resp).await;

        let req = test::TestRequest::put()
            .uri("/api/pipe/2")
            .append_header(auth.clone())
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let _: model::CollectResponse = test::read_body_json(resp).await;

        let req = test::TestRequest::post()
            .uri("/api/pipe/3/modifier")
            .append_header(auth.clone())
            .set_json(ApplyModifierInput {
                modifier: model::Modifier::Double,
            })
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let _: model::ApplyModifierResponse = test::read_body_json(resp).await;
    }
}
