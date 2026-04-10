# XandSuite — headless server image
# Runs the Tauri backend without a display, serving the HTTP API on port 3847.
# The built React frontend (dist/) is served statically from the same process.
#
# Build:
#   npm run build                          # build frontend
#   cargo tauri build                      # build Tauri binary
#   docker build -t xandsuite .
#
# Run:
#   docker run -p 3847:3847 -v xand_data:/data xandsuite

FROM ubuntu:22.04

# Tauri / WebKit runtime dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    libwebkit2gtk-4.1-0 \
    libgtk-3-0 \
    libayatana-appindicator3-1 \
    librsvg2-bin \
    libssl3 \
    ca-certificates \
    python3 \
    python3-pip \
    curl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy the built binary and frontend assets
COPY src-tauri/target/release/xandsuite   /app/xandsuite
COPY dist/                                 /app/dist/
COPY tools/                               /app/tools/

# Copy installer binaries so the /api/download endpoint can serve them
COPY version/executables/                  /app/installers/

RUN chmod +x /app/xandsuite

# Data directory (SQLite DB, models, cache) — mount a volume here for persistence
ENV XDG_DATA_HOME=/data
RUN mkdir -p /data

# Tell XandSuite to run without a window
ENV XANDSUITE_HEADLESS=1

# Serve the frontend from the dist/ directory next to the binary
ENV XANDSUITE_FRONTEND_DIST=/app/dist

# Point Python packages to the bundled tools directory
ENV XANDSUITE_TOOLS_DIR=/app/tools

# Installer binaries served by /api/download
ENV XANDSUITE_INSTALLERS_DIR=/app/installers

EXPOSE 3847

CMD ["/app/xandsuite"]
