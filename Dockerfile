# Use Debian as the base image
FROM debian:bullseye
#FROM ubuntu:20.04

# Avoid prompts from apt
ENV DEBIAN_FRONTEND=noninteractive

# Install required packages and Redis
# redis-server redis-tools
RUN apt-get update && \
    apt-get install -y nfs-common curl build-essential pkg-config libssl-dev npm && \
    rm -rf /var/lib/apt/lists/*

RUN npm install -g wscat

# Install Rust
#RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y

# Set the PATH environment variable to include the Cargo bin directory
#ENV PATH="/root/.cargo/bin:${PATH}"

# Copy the pre-compiled graymamba binary instead of source code
COPY target/x86_64-unknown-linux-gnu/debug/graymamba /usr/local/bin/graymamba
# Make the graymamba executable
RUN chmod +x /usr/local/bin/graymamba

# Expose the necessary ports
EXPOSE 2049

# Set the entry point to launch the NFS server
ENTRYPOINT ["/usr/local/bin/graymamba", "/mnt/nfs"]
