# Build the app, then run it from a small image. Views, translations and
# public files are compiled into the binary (renox::embedded!()).
#
# cargo-chef builds the dependencies in their own layer, which Docker reuses
# until Cargo.toml or Cargo.lock change: a code change rebuilds only the app.
FROM lukemathwalker/cargo-chef:latest-rust-1-bookworm AS chef
WORKDIR /src

FROM chef AS plan
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS build
COPY --from=plan /src/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release && cp target/release/bikeshop /bikeshop

FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --home /app app
WORKDIR /app
COPY --from=build /bikeshop /app/bikeshop
ENV APP_ENV=production \
    APP_DEBUG=false \
    APP_HOST=0.0.0.0 \
    APP_PORT=3000 \
    STORAGE_PATH=/app/storage \
    DATABASE_URL=sqlite:///app/storage/app.db
RUN mkdir -p /app/storage && chown app /app/storage
USER app
VOLUME /app/storage
EXPOSE 3000
# Pending migrations run on every start; `docker run -e APP_KEY=...` is required.
CMD ["sh", "-c", "/app/bikeshop migrate && exec /app/bikeshop"]
