# Node.js 24.21.0 on Debian 13 (trixie); the multi-platform index is immutable.
FROM node:24.21.0-trixie-slim@sha256:8ec5d7557396cfe32d21c3f9c13072355ceab22b584578ca4bb28af31120cffe AS build
RUN npm install --global corepack@0.32.0
WORKDIR /build/web
COPY web/package.json web/pnpm-lock.yaml web/pnpm-workspace.yaml ./
RUN corepack pnpm install --frozen-lockfile
COPY web ./
COPY brand /build/brand
ENV NEXT_TELEMETRY_DISABLED=1
RUN corepack pnpm build
# Reproducible layers: the copied output carries one fixed timestamp instead of the build time.
# BuildKit also uses this argument for the image's creation time and history.
ARG SOURCE_DATE_EPOCH=0
RUN find .next/standalone .next/static public -exec touch -h -d "@${SOURCE_DATE_EPOCH}" {} +

# The runtime runs `node server.js` and nothing else, so it ships Node alone: no npm, npx, corepack or Yarn, not even
# in a lower layer (dependency audit A4). The same Debian 13 base as the runtime images.
FROM debian:trixie-slim@sha256:62bd423aeee912639ca8ee681c2b762cb3e25137e0a92b20be455898a9f034ad
COPY --from=build /usr/local/bin/node /usr/local/bin/node
COPY --from=build /usr/local/LICENSE /usr/share/doc/node/LICENSE
WORKDIR /app
ENV NODE_ENV=production NEXT_TELEMETRY_DISABLED=1 HOSTNAME=0.0.0.0 PORT=3000
COPY --from=build --chown=1000:1000 /build/web/.next/standalone ./
COPY --from=build --chown=1000:1000 /build/web/.next/static ./.next/static
COPY --from=build --chown=1000:1000 /build/web/public ./public
USER 1000:1000
EXPOSE 3000
HEALTHCHECK --interval=5s --timeout=3s --start-period=30s --retries=12 CMD ["node", "-e", "fetch('http://127.0.0.1:3000/login',{signal:AbortSignal.timeout(2000)}).then(r=>process.exit(r.ok?0:1)).catch(()=>process.exit(1))"]
CMD ["node", "server.js"]
