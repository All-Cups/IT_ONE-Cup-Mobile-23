use crate::serde_duration;
use actix_web::rt::time::sleep;
use async_mutex::{Mutex, MutexGuardArc};
use futures::{channel::mpsc, SinkExt};
use log::{debug, error, info, warn};
use rand::{seq::SliceRandom, thread_rng, Rng};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    fmt::Debug,
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant},
};

pub type Score = i64;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub reverse_cost: Score,
    pub double_cost: Score,
    pub double_uses: usize,
    pub slow_cost: Score,
    pub slow_uses: usize,
    pub shuffle_cost: Score,
    pub min_cost: Score,
    pub min_uses: usize,
    pub pipe_count: usize,
    pub min_value: Score,
    pub max_value: Score,
    pub min_delay_secs: f64,
    pub max_delay_secs: f64,
    pub pipe_value_delay_secs: f64,
    pub time_to_run: Option<f64>,
}

impl Default for Config {
    fn default() -> Self {
        serde_json::from_str(include_str!("../config.json"))
            .expect("Failed to parse default config")
    }
}

impl Config {
    pub fn modifier_cost(&self, modifier: Modifier) -> Score {
        match modifier {
            Modifier::Slow => self.slow_cost,
            Modifier::Double => self.double_cost,
            Modifier::Min => self.min_cost,
            Modifier::Shuffle => self.shuffle_cost,
            Modifier::Reverse => self.reverse_cost,
        }
    }
    pub fn random_pipe_delay(&self) -> Duration {
        Duration::from_secs_f64(thread_rng().gen_range(self.min_delay_secs..=self.max_delay_secs))
    }
    pub fn random_pipe_value(&self) -> Score {
        thread_rng().gen_range(self.min_value..=self.max_value)
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub struct UserToken(String);

impl From<String> for UserToken {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl std::borrow::Borrow<String> for UserToken {
    fn borrow(&self) -> &String {
        &self.0
    }
}

impl Debug for UserToken {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for UserToken {
    type Err = <String as FromStr>::Err;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(Self(String::from_str(s)?))
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct User {
    pub score: Score,
}

#[derive(Debug, Serialize, Deserialize, Copy, Clone, PartialEq, Eq, Hash)]
pub enum PipeDirection {
    Up,
    Down,
}
impl PipeDirection {
    pub fn inverse(&self) -> Self {
        match self {
            Self::Up => Self::Down,
            Self::Down => Self::Up,
        }
    }

    pub fn random() -> PipeDirection {
        *[Self::Up, Self::Down].choose(&mut thread_rng()).unwrap()
    }
}

#[derive(Debug, Serialize, Deserialize, Copy, Clone, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Modifier {
    Slow,
    Double,
    Min,
    Shuffle,
    Reverse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pipe {
    pub value: Score,
    #[serde(with = "serde_duration")]
    pub base_delay: Duration,
    pub direction: PipeDirection,
    pub modifiers: HashMap<Modifier, usize>,
}

impl Pipe {
    #[must_use]
    pub fn use_modifier(&mut self, modifier: Modifier) -> bool {
        let Some(uses_left) = self.modifiers.get_mut(&modifier) else { return false };
        assert_ne!(*uses_left, 0);
        *uses_left -= 1;
        debug!("Using {modifier:?} modifier, {uses_left} uses left now");
        if *uses_left == 0 {
            debug!("{modifier:?} is now removed from the pipe");
            self.modifiers.remove(&modifier);
        }
        true
    }
}

pub struct App {
    start: Instant,
    allow_unknown_users: bool,
    config: Config,
    users: Mutex<HashMap<UserToken, Arc<Mutex<User>>>>,
    pipes: HashMap<usize, Mutex<Pipe>>,
    log_senders: Mutex<Vec<mpsc::UnboundedSender<LogEntry>>>,
    history: Mutex<Vec<LogEntry>>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum LogMessage<U = UserToken> {
    CollectStart {
        user: U,
        pipe_id: usize,
        #[serde(with = "serde_duration")]
        delay: Duration,
    },
    UpdatePipe {
        id: usize,
        #[serde(flatten)]
        state: Pipe,
    },
    CollectEnd {
        user: U,
    },
    UpdateUser {
        user: U,
        #[serde(flatten)]
        state: User,
    },
}

impl<U> LogMessage<U> {
    pub fn map_user<V>(self, f: impl Fn(U) -> V) -> LogMessage<V> {
        match self {
            LogMessage::CollectStart {
                user,
                pipe_id,
                delay,
            } => LogMessage::CollectStart {
                user: f(user),
                pipe_id,
                delay,
            },
            LogMessage::UpdatePipe { id, state } => LogMessage::UpdatePipe { id, state },
            LogMessage::CollectEnd { user } => LogMessage::CollectEnd { user: f(user) },
            LogMessage::UpdateUser { user, state } => LogMessage::UpdateUser {
                user: f(user),
                state,
            },
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct LogEntry<U = UserToken> {
    pub time: f64,
    pub msg: LogMessage<U>,
}

impl<U> LogEntry<U> {
    pub fn map_user<V>(self, f: impl Fn(U) -> V) -> LogEntry<V> {
        LogEntry {
            time: self.time,
            msg: self.msg.map_user(f),
        }
    }
}

impl App {
    async fn log(&self, msg: LogMessage) {
        let entry = LogEntry {
            time: self.start.elapsed().as_secs_f64(),
            msg,
        };
        let mut senders = self.log_senders.lock().await;
        for sender in senders.iter_mut() {
            if let Err(e) = sender.send(entry.clone()).await {
                error!("{e}");
            }
        }
        self.history.lock().await.push(entry);
    }
    pub async fn register_logs(&self, mut sender: mpsc::UnboundedSender<LogEntry>) {
        for msg in self.history.lock().await.iter() {
            if let Err(e) = sender.send(msg.clone()).await {
                error!("{e}");
                return;
            }
        }
        self.log_senders.lock().await.push(sender);
    }
    pub async fn unregister_logs(&self, sender: &mpsc::UnboundedSender<LogEntry>) {
        self.log_senders
            .lock()
            .await
            .retain(|s| !s.same_receiver(sender));
    }
}

pub type Results = BTreeMap<String, Score>;

impl App {
    pub async fn results(&self) -> Results {
        let mut result = BTreeMap::new();
        for (token, user) in self.users.lock().await.iter() {
            result.insert(token.0.clone(), user.lock().await.score);
        }
        result
    }
}

#[derive(thiserror::Error, Serialize, Debug)]
pub enum Error {
    #[error("User not found")]
    UserNotFound,
    #[error("User is already processing another request")]
    UserBusy,
    #[error("Pipe not found")]
    PipeNotFound,
    #[error("Not enough score")]
    NotEnoughScore,
    #[error("This modifier is already applied to the pipe")]
    ModifierAlreadyApplied,
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

impl App {
    async fn try_lock_user(&self, token: &UserToken) -> Result<MutexGuardArc<User>> {
        let mut users = self.users.lock().await;
        let user = if self.allow_unknown_users {
            // Create new user on demand
            users.entry(token.to_owned()).or_insert_with(|| {
                info!("Unknown user detected, creating {token:?}");
                Default::default()
            })
        } else {
            users.get(token).ok_or_else(|| {
                warn!("Someone tried to use the api with incorrect token: {token:?}");
                Error::UserNotFound
            })?
        };
        user.try_lock_arc().ok_or(Error::UserBusy)
    }

    fn pipe(&self, id: usize) -> Result<&Mutex<Pipe>> {
        self.pipes.get(&id).ok_or(Error::PipeNotFound)
    }
}

impl App {
    pub fn init(config: Config, users: impl IntoIterator<Item = UserToken>) -> Self {
        let users: Vec<UserToken> = users.into_iter().collect();
        debug!("Initializing app...");
        info!("Config: {config:#?}");
        let allow_unknown_users = users.is_empty();
        if allow_unknown_users {
            info!("No users specified, so everyone is welcome");
        } else {
            info!("Users: {users:#?}");
        }
        let mut history = Vec::new();
        let users = Mutex::new(
            users
                .into_iter()
                .map(|token| {
                    let user: User = Default::default();
                    history.push(LogEntry {
                        time: 0.0,
                        msg: LogMessage::UpdateUser {
                            user: token.clone(),
                            state: user.clone(),
                        },
                    });
                    (token, Arc::new(Mutex::new(user)))
                })
                .collect(),
        );
        let pipes = (1..=config.pipe_count)
            .map(|id| {
                let pipe = Pipe {
                    value: config.random_pipe_value(),
                    base_delay: config.random_pipe_delay(),
                    direction: PipeDirection::random(),
                    modifiers: HashMap::new(),
                };
                debug!("Pipe #{id}: {pipe:#?}");
                history.push(LogEntry {
                    time: 0.0,
                    msg: LogMessage::UpdatePipe {
                        id,
                        state: pipe.clone(),
                    },
                });
                (id, Mutex::new(pipe))
            })
            .collect();
        Self {
            start: Instant::now(),
            allow_unknown_users,
            users,
            pipes,
            config,
            log_senders: Default::default(),
            history: Mutex::new(history),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct PipeValueResponse {
    pub value: Score,
}

impl App {
    pub async fn pipe_value(
        &self,
        user_token: &UserToken,
        pipe_id: usize,
    ) -> Result<PipeValueResponse> {
        let _user = self.try_lock_user(user_token).await?;
        let pipe = self.pipe(pipe_id)?;
        info!("User {user_token:?} is finding out value of pipe {pipe_id}");
        let delay = Duration::from_secs_f64(self.config.pipe_value_delay_secs);
        debug!("Sleeping for {delay:?}");
        sleep(delay).await;
        let value = pipe.lock().await.value;
        debug!("Sleep finished, {user_token:?} now knows pipe {pipe_id} value: {value}");
        Ok(PipeValueResponse { value })
    }
}

#[derive(Serialize, Deserialize)]
pub struct CollectResponse {
    pub value: Score,
}

impl App {
    pub async fn collect(&self, user_token: &UserToken, pipe_id: usize) -> Result<CollectResponse> {
        let mut user = self.try_lock_user(user_token).await?;
        let pipe = self.pipe(pipe_id)?;
        info!("User {user_token:?} is trying to collect pipe {pipe_id}");
        debug!("Pipe state: {:#?}", pipe.lock().await);
        let delay = {
            let mut pipe = pipe.lock().await;
            let mut delay = pipe.base_delay;
            if pipe.use_modifier(Modifier::Slow) {
                delay *= 2;
            }
            self.log(LogMessage::UpdatePipe {
                id: pipe_id,
                state: pipe.clone(),
            })
            .await;
            delay
        };
        self.log(LogMessage::CollectStart {
            user: user_token.clone(),
            pipe_id,
            delay,
        })
        .await;
        debug!("Sleeping for {delay:?}");
        sleep(delay).await;
        self.log(LogMessage::CollectEnd {
            user: user_token.clone(),
        })
        .await;
        debug!(
            "Sleep finished, {user_token:?} is now going to collect from pipe {pipe_id}: {:#?}",
            pipe.lock().await,
        );
        let mut pipe = pipe.lock().await;
        let score = {
            let mut score = pipe.value;
            if pipe.use_modifier(Modifier::Double) {
                score *= 2;
            }
            // TODO: what if both Min & Double are present? Maybe Double should not be used up?
            if pipe.use_modifier(Modifier::Min) {
                score = self.config.min_value;
            }
            score
        };
        debug!("Score retrieved from the pipe: {score}");
        user.score += score;
        debug!("User's score is now {}", user.score);
        pipe.value += match pipe.direction {
            PipeDirection::Up => 1,
            PipeDirection::Down => -1,
        };
        if pipe.value < self.config.min_value {
            pipe.value = self.config.max_value;
        } else if pipe.value > self.config.max_value {
            pipe.value = self.config.min_value;
        }
        debug!("Next pipe value will be {}", pipe.value);
        self.log(LogMessage::UpdatePipe {
            id: pipe_id,
            state: pipe.clone(),
        })
        .await;
        self.log(LogMessage::UpdateUser {
            user: user_token.clone(),
            state: user.clone(),
        })
        .await;
        Ok(CollectResponse { value: score })
    }
}

#[derive(Serialize, Deserialize)]
pub struct ApplyModifierResponse {}

impl App {
    pub async fn apply_modifier(
        &self,
        user_token: &UserToken,
        pipe_id: usize,
        modifier: Modifier,
    ) -> Result<ApplyModifierResponse> {
        let mut user = self.try_lock_user(user_token).await?;
        let mut pipe = self.pipe(pipe_id)?.lock().await;
        info!(
            "User {user_token:?}: {user:?} is trying apply {modifier:?} modifier to pipe {pipe_id}"
        );
        debug!("Pipe state: {pipe:#?}");
        let cost = self.config.modifier_cost(modifier);
        if user.score < cost {
            debug!("Not enough score to pay for modification");
            return Err(Error::NotEnoughScore);
        }
        match modifier {
            Modifier::Slow | Modifier::Double | Modifier::Min => {
                if pipe.modifiers.contains_key(&modifier) {
                    debug!("Modifier already applied");
                    return Err(Error::ModifierAlreadyApplied);
                }
                let uses = match modifier {
                    Modifier::Slow => self.config.slow_uses,
                    Modifier::Double => self.config.double_uses,
                    Modifier::Min => self.config.min_uses,
                    _ => unreachable!("Well, we just checked its one of these"),
                };
                debug!("Adding {modifier:?} modifier to pipe {pipe_id} with {uses} uses");
                pipe.modifiers.insert(modifier, uses);
            }
            Modifier::Shuffle => {
                pipe.base_delay = self.config.random_pipe_delay();
                debug!("Pipe's base delay changed to {:?}", pipe.base_delay);
            }
            Modifier::Reverse => {
                pipe.direction = pipe.direction.inverse();
                debug!("Pipe's new direction is {:?}", pipe.direction);
            }
        }
        user.score -= cost;
        debug!("User's score is now {}", user.score);
        self.log(LogMessage::UpdateUser {
            user: user_token.clone(),
            state: user.clone(),
        })
        .await;
        self.log(LogMessage::UpdatePipe {
            id: pipe_id,
            state: pipe.clone(),
        })
        .await;
        Ok(ApplyModifierResponse {})
    }
}
