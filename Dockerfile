FROM node:24-bookworm-slim AS base
WORKDIR /app

FROM base AS dependencies
COPY package.json yarn.lock ./
RUN yarn install --frozen-lockfile

FROM dependencies AS build
COPY . .
RUN yarn build && rm -rf build/tests build/bin/test.js build/bin/test.js.map

FROM base AS production-dependencies
COPY package.json yarn.lock ./
RUN yarn install --production --frozen-lockfile && yarn cache clean

FROM base AS runtime
ENV NODE_ENV=production \
		HOST=0.0.0.0 \
		PORT=8080 \
		TZ=UTC \
		LOG_LEVEL=info \
		SESSION_DRIVER=cookie \
		LIMITER_STORE=database \
		DB_PORT=5432 \
		DB_SSL_MODE=verify-full \
		DB_POOL_MAX=5
COPY --from=production-dependencies /app/node_modules ./node_modules
COPY --from=build /app/build ./
RUN mkdir -p tmp && chown node:node tmp
USER node
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
	CMD node -e "fetch('http://127.0.0.1:'+process.env.PORT+'/health/live',{signal:AbortSignal.timeout(4000)}).then(r=>process.exit(r.ok?0:1)).catch(()=>process.exit(1))"
CMD ["node", "bin/server.js"]