# Use Debian as the base image
FROM debian:latest

# Avoid prompts from apt
ENV DEBIAN_FRONTEND=noninteractive

# Install nfs-common package
RUN apt-get update && \
    apt-get install -y nfs-common && \
    rm -rf /var/lib/apt/lists/*

# Create the mount point directory
RUN mkdir -p /mnt/mule

# Copy the mirrorfs executable into the image
COPY target/x86_64-unknown-linux-gnu/release/examples/mirrorfs /usr/local/bin/mirrorfs

# Make the mirrorfs executable
RUN chmod +x /usr/local/bin/mirrorfs

# Expose port 11111 for mirrorfs
EXPOSE 11111

# Command to run when the container starts
CMD ["/usr/local/bin/mirrorfs", "/mnt/mule"]