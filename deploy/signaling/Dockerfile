# matchbox signaling server — tiny stateless websocket introducer
FROM rust:1-slim AS builder
RUN cargo install matchbox_server --version 0.14.0 --locked

FROM debian:bookworm-slim
COPY --from=builder /usr/local/cargo/bin/matchbox_server /usr/local/bin/
EXPOSE 3536
CMD ["matchbox_server"]
