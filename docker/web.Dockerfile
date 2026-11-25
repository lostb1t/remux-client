# The web app, built by `nsh package --target web` (the workflow `Build Web`, artifact `web-dist`,
# downloaded to web-dist/), served by static-web-server: the app is static files.
FROM ghcr.io/static-web-server/static-web-server:2-debian

COPY web-dist /public

ENV SERVER_PORT=80
ENV SERVER_HOST=0.0.0.0
ENV SERVER_ROOT=/public
# The .br and .gz copies nsh writes next to each file, for browsers that take them: the runtime is
# about 13 MB as it is, 3.3 MB with Brotli.
ENV SERVER_COMPRESSION_STATIC=true
# The app's own paths are its pages: index.html for them (the app reads the address).
ENV SERVER_FALLBACK_PAGE=/public/index.html
