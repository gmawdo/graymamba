# Use Debian as the base image
FROM debian:bullseye

# Avoid prompts from apt
ENV DEBIAN_FRONTEND=noninteractive

# Install required packages
RUN apt-get update && \
    apt-get install -y nfs-common curl build-essential pkg-config libssl-dev npm && \
    rm -rf /var/lib/apt/lists/*

RUN npm install -g wscat

# Copy the cross-compiled graymamba binary
COPY target/x86_64-unknown-linux-gnu/release/graymamba /usr/local/bin/graymamba

# Make the graymamba binary, executable
RUN chmod +x /usr/local/bin/graymamba

# Expose the necessary port
EXPOSE 2049

# Set the entry point to launch the NFS server
ENTRYPOINT ["/usr/local/bin/graymamba"]
