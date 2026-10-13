# Laravel for the benchmark, in production mode. Two targets from one build:
#   fpm:    PHP-FPM behind nginx (serversideup/php, OPcache on), the usual deploy
#   octane: Laravel Octane on FrankenPHP, Laravel's fastest server
# Build from benchmarks/ (the context holds data/bench.db): docker build -f laravel/Dockerfile --target fpm .
# The app is a fresh `laravel/laravel` with the benchmark's controller, routes
# and view on top (overlay/), config, routes, events and views cached.
ARG LARAVEL=^12.0

FROM composer:2 AS app
ARG LARAVEL
WORKDIR /build
RUN composer create-project laravel/laravel app "$LARAVEL" --no-dev --prefer-dist --no-interaction --no-scripts \
 && cd app && composer require laravel/octane --no-interaction --no-scripts --update-no-dev
COPY laravel/overlay/ /build/app/
COPY laravel/env.production /build/app/.env

FROM serversideup/php:8.4-fpm-nginx AS fpm
ENV PHP_OPCACHE_ENABLE=1
COPY --from=app --chown=www-data:www-data /build/app /var/www/html
COPY --chown=www-data:www-data data/bench.db /var/www/html/database/bench.db
RUN php artisan key:generate --force && php artisan optimize

FROM serversideup/php:8.4-frankenphp AS octane
ENV PHP_OPCACHE_ENABLE=1
COPY --from=app --chown=www-data:www-data /build/app /var/www/html
COPY --chown=www-data:www-data data/bench.db /var/www/html/database/bench.db
RUN php artisan key:generate --force && php artisan optimize
CMD ["php", "artisan", "octane:start", "--server=frankenphp", "--host=0.0.0.0", "--port=8080", "--workers=auto", "--max-requests=100000"]
