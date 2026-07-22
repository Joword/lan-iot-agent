# syntax=docker/dockerfile:1

FROM node:22-alpine AS deps
WORKDIR /app
COPY package.json ./
RUN npm install

FROM node:22-alpine AS builder
WORKDIR /app
COPY --from=deps /app/node_modules ./node_modules
COPY . .
ENV NEXT_TELEMETRY_DISABLED=1
# Browser-facing Hub URL (host port). Do not use Docker service DNS here.
ARG NEXT_PUBLIC_HUB_URL=http://localhost:3000
ARG NEXT_PUBLIC_AGENT_URL=http://localhost:8000
ENV NEXT_PUBLIC_HUB_URL=$NEXT_PUBLIC_HUB_URL
ENV NEXT_PUBLIC_AGENT_URL=$NEXT_PUBLIC_AGENT_URL
ENV HUB_URL=$NEXT_PUBLIC_HUB_URL
ENV AGENT_URL=$NEXT_PUBLIC_AGENT_URL
RUN npm run build

FROM node:22-alpine AS runner
WORKDIR /app
ENV NODE_ENV=production
ENV NEXT_TELEMETRY_DISABLED=1
ENV NEXT_PUBLIC_HUB_URL=http://localhost:3000
ENV NEXT_PUBLIC_AGENT_URL=http://localhost:8000
ENV HUB_URL=http://localhost:3000
ENV AGENT_URL=http://localhost:8000
RUN mkdir -p public
COPY --from=builder /app/public ./public
COPY --from=builder /app/.next/standalone ./
COPY --from=builder /app/.next/static ./.next/static
EXPOSE 3000
CMD ["node", "server.js"]
