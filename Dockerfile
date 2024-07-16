# Use Debian as the base image
FROM debian:latest

# Avoid prompts from apt
ENV DEBIAN_FRONTEND=noninteractive

# Install required packages and Redis
RUN apt-get update && \
    apt-get install -y nfs-common curl build-essential pkg-config libssl-dev redis-server redis-tools && \
    rm -rf /var/lib/apt/lists/*

# Install Rust
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y

# Set the PATH environment variable to include the Cargo bin directory
ENV PATH="/root/.cargo/bin:${PATH}"

# Create the necessary directories
RUN mkdir -p /mnt/nfs /mount_point /app/Redis_database/redis-6380 /app/Redis_database/redis-6381 /app/Redis_database/redis-6382

# Copy the source code into the image
COPY . /app
WORKDIR /app

# Build the Rust project
RUN cargo build --bin lockular_nfs --features="demo" --release

# Copy the built binary to a location in the PATH
RUN cp /app/target/release/lockular_nfs /usr/local/bin/lockular_nfs

# Make the lockular_nfs executable
RUN chmod +x /usr/local/bin/lockular_nfs

# Expose the necessary ports
EXPOSE 2049 9944 6380 6381 6382

# Copy and make the entrypoint script executable
COPY entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

# Command to run when the container starts
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
