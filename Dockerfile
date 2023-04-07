FROM node AS frontend-builder
WORKDIR /src
COPY frontend .
RUN npm install && npm run build

FROM rust:bullseye AS backend-builder
WORKDIR /src
# First create a layer with built dependencies to cache them in separate layer
COPY Cargo.toml .
RUN mkdir src && touch src/lib.rs && cargo build --release && rm -rf src
# Now actually compile the project
COPY . .
RUN cargo build --release

# Now create a small image
FROM debian:bullseye-slim
WORKDIR /root
COPY --from=backend-builder /src/target/release/itonecup-mobile .
COPY --from=frontend-builder /src/dist frontend
ENTRYPOINT ["./itonecup-mobile", "--serve-dir=frontend", "--addr=0.0.0.0:80"]
EXPOSE 80