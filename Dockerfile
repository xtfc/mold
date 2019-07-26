FROM ekidd/rust-musl-builder:1.36.0 AS build
COPY Cargo.toml ./
COPY src ./src
RUN sudo chown -R rust:rust /home/rust \
 && cargo build --release \
 && cp /home/rust/src/target/x86_64-unknown-linux-musl/release/mold /home/rust/mold
