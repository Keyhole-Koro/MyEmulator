# Use a base image
FROM debian:bookworm

# Install required dependencies
RUN apt update && apt install -y \
    build-essential \
    cmake \
    git \
    libgtest-dev \
    && rm -rf /var/lib/apt/lists/*

# Clone, build, and install Google Test
RUN git clone https://github.com/google/googletest.git /tmp/googletest && \
    cd /tmp/googletest && \
    cmake -S . -B build && \
    cmake --build build && \
    cd build && \
    make install && \
    rm -rf /tmp/googletest

# Set the working directory
WORKDIR /workspaces/YourVM
