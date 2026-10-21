# syntax=docker/dockerfile:1
FROM rust:1-slim-bookworm AS build
WORKDIR /src
COPY server/Cargo.toml server/Cargo.lock ./server/
COPY server/src ./server/src
RUN cd server && cargo build --release --locked

FROM gcr.io/distroless/cc-debian12:nonroot
WORKDIR /app
COPY --from=build /src/server/target/release/five-trails-server /app/five-trails-server
COPY web /app/web
ENV FT_BIND=0.0.0.0:8080 FT_WEB_DIR=/app/web RUST_LOG=info
EXPOSE 8080
USER nonroot:nonroot
ENTRYPOINT ["/app/five-trails-server"]
