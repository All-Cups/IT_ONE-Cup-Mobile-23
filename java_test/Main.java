package java_test;

import java.io.IOException;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.util.Random;

public class Main {

    static final int PIPES = 3;

    public static void main(String[] args) {
        String host = args.length >= 1 ? args[0] : "127.0.0.1:8080";
        String token = args.length >= 2 ? args[1] : "JavaQuickstart";
        System.out.println("host = " + host + ", token = " + token);
        Main main = new Main(host, token);
        main.run();
    }

    enum Mod {
        SLOW("slow"),
        DOUBLE("double"),
        MIN("min"),
        SHUFFLE("shuffle"),
        REVERSE("reverse"),

        ;

        final String name;

        Mod(String name) {
            this.name = name;
        }
    }

    final String url;
    final String auth;

    final HttpClient client;
    final Random random = new Random(0);

    Main(String host, String token) {
        this.url = "http://" + host + "/api";
        this.auth = "Bearer " + token;
        this.client = HttpClient.newHttpClient();
    }

    void run() {
        for (;;) {
            try {
                switch (random.nextInt(3)) {
                    case 0 -> collect();
                    case 1 -> value();
                    case 2 -> modifier();
                }
            } catch (IOException | InterruptedException e) {
                e.printStackTrace();
                break;
            }
        }
    }

    void collect() throws IOException, InterruptedException {
        int pipe = 1 + random.nextInt(PIPES);
        HttpRequest request = HttpRequest.newBuilder()
                .PUT(HttpRequest.BodyPublishers.noBody())
                .uri(URI.create(url + "/pipe/" + pipe))
                .header("Authorization", auth)
                .build();
        HttpResponse<String> resp = client.send(request, HttpResponse.BodyHandlers.ofString());
        if (resp.statusCode() == 200) {
            System.out.println("collect " + pipe + " -> " + resp.body());
        } else {
            System.out.println("collect " + pipe + " -> " + resp.statusCode());
        }
    }

    void value() throws IOException, InterruptedException {
        int pipe = 1 + random.nextInt(PIPES);
        HttpRequest request = HttpRequest.newBuilder()
                .GET()
                .uri(URI.create(url + "/pipe/" + pipe + "/value"))
                .header("Authorization", auth)
                .build();
        HttpResponse<String> resp = client.send(request, HttpResponse.BodyHandlers.ofString());
        if (resp.statusCode() == 200) {
            System.out.println("value " + pipe + " -> " + resp.body());
        } else {
            System.out.println("value " + pipe + " -> " + resp.statusCode());
        }
    }

    void modifier() throws IOException, InterruptedException {
        int pipe = 1 + random.nextInt(PIPES);
        Mod mod = Mod.values()[random.nextInt(Mod.values().length)];
        HttpRequest request = HttpRequest.newBuilder()
                .POST(HttpRequest.BodyPublishers.ofString("{\"type\": \"" + mod.name + "\"}"))
                .uri(URI.create(url + "/pipe/" + pipe + "/modifier"))
                .header("Authorization", auth)
                .header("Content-Type", "application/json")
                .build();
        HttpResponse<String> resp = client.send(request, HttpResponse.BodyHandlers.ofString());
        if (resp.statusCode() == 200) {
            System.out.println("modifier " + pipe + " -> " + resp.body());
        } else {
            System.out.println("modifier " + pipe + " -> " + resp.statusCode());
        }
    }
}
