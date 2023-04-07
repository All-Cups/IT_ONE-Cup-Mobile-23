# IT_ONE Cup - Mobile 23

## Сборка и запуск с использованием докера

Сборка:

```sh
docker build -t itonecup-mobile .
```

Запуск:

```sh
docker run --rm -it -p 8080:80 itonecup-mobile
```

## Сборка и запуск из исходников без докера

Нужен компилятор Rust - <https://rustup.rs>

Запуск:

```sh
cargo run --release
```

Полный список аргументов командной строки можно посмотреть с помощью `cargo run --release -- --help`

## Пример запросов

[Пример на Python](clients/python/)

