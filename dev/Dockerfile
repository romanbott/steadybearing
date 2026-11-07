# Build a self-contained fake-sysfs harness for sb-daemon.
#
#   docker build -f dev/Dockerfile -t sb-harness .
#   docker run --rm -it sb-harness sb        # TUI against the simulated daemon
#   docker run --rm sb-harness               # headless
FROM rust:1-slim-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release -p sb-daemon --features test-hwmon \
    && cargo build --release -p sb

FROM debian:bookworm-slim
COPY --from=build /src/target/release/sb-daemon /usr/local/bin/sb-daemon
COPY --from=build /src/target/release/sb /usr/local/bin/sb
COPY dev/plant.sh /usr/local/bin/plant.sh
COPY dev/entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/plant.sh /usr/local/bin/entrypoint.sh
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
