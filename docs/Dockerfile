# Multi-stage Dockerfile for RA documentation site

# Stage 1: Build documentation
FROM node:22-alpine AS builder

WORKDIR /docs

# Copy package files
COPY package.json package-lock.json ./

# Install dependencies
RUN npm ci --production=false

# Copy documentation source
COPY . .

# Build VitePress documentation site
RUN npm run build

# Stage 2: Production server
FROM nginx:1.27-alpine

# Copy built documentation from builder
COPY --from=builder /docs/.vitepress/dist /usr/share/nginx/html

# Copy nginx configuration
COPY nginx.conf /etc/nginx/conf.d/default.conf

# Add healthcheck
HEALTHCHECK --interval=30s --timeout=3s --retries=3 \
  CMD wget --quiet --tries=1 --spider http://localhost/ || exit 1

EXPOSE 80

CMD ["nginx", "-g", "daemon off;"]
