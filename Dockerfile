# Use a base image
FROM debian:bookworm

# Install required dependencies
RUN apt update && apt install -y \
    build-essential \
    cmake \
    git \
    curl \
    libgtest-dev \
    tar \
    gdb \
    && rm -rf /var/lib/apt/lists/*

# Install Google Test
RUN git clone https://github.com/google/googletest.git /tmp/googletest && \
    cd /tmp/googletest && \
    cmake -S . -B build && \
    cmake --build build && \
    cmake --install build && \
    rm -rf /tmp/googletest

# Install act (locally to /usr/local/bin)
RUN curl -L https://github.com/nektos/act/releases/latest/download/act_Linux_x86_64.tar.gz | \
    tar -xz -C /usr/local/bin && \
    chmod +x /usr/local/bin/act

# Set the working directory
WORKDIR /workspaces/MyEmulator
