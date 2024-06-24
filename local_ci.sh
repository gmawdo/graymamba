#!/bin/bash

# Define variables
IMAGE_NAME="datasignals/sptfs"
TAG="local-$(date +'%Y%m%d-%H%M%S')"
FULL_IMAGE_NAME="${IMAGE_NAME}:${TAG}"

# Step 1: Build Docker image
echo "Building Docker image..."
docker build -t $FULL_IMAGE_NAME .

if [ $? -ne 0 ]; then
    echo "Docker build failed."
    exit 1
fi

# Step 2: Setup environment
echo "Setting up environment..."
bash ./tests/setup.sh

if [ $? -ne 0 ]; then
    echo "Environment setup failed."
    exit 1
fi

# Step 3: Run tests
echo "Running tests..."
docker run -v "$(pwd)/tests:/mount_point/tests" $FULL_IMAGE_NAME /bin/bash -c "/mount_point/tests/basic.sh" &&
docker run -v "$(pwd)/tests:/mount_point/tests" $FULL_IMAGE
_NAME /bin/bash -c "/mount_point/tests/intermediate.sh" &&
docker run -v "$(pwd)/tests:/mount_point/tests" $FULL_IMAGE_NAME /bin/bash -c "/mount_point/tests/advance.sh"
if [ $? -eq 0 ]; then
    echo "Tests passed. Pushing Docker image..."
    # Step 4: Push Docker image
    docker login -u "$DOCKER_USERNAME" -p "$DOCKER_PASSWORD"
    docker push $FULL_IMAGE_NAME
    echo "Docker image pushed: $FULL_IMAGE_NAME"
else
    echo "Tests failed. Not pushing Docker image."
    exit 1
fi