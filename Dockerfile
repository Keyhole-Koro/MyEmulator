# Dockerfile
FROM gcc:13

# Install additional tools
RUN apt-get update && apt-get install -y \
    make \
    gdb \
    git \
    cmake \
    && rm -rf /var/lib/apt/lists/*

# Set working directory
WORKDIR /workspace

# This container is for dev; building/running is done via VS Code tasks
