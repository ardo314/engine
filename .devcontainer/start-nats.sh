#!/usr/bin/env bash
# Start NATS server with JetStream for development.
# Used by devcontainer postStartCommand.

if timeout 1 bash -c 'exec 3<>/dev/tcp/127.0.0.1/4222' 2>/dev/null; then
    echo "NATS server already running on :4222"
    exit 0
fi

echo "Starting NATS server with JetStream..."
nats-server -js > /tmp/nats.log 2>&1 &

# Wait for it to be ready (up to 5 seconds)
for i in $(seq 1 50); do
    if timeout 1 bash -c 'exec 3<>/dev/tcp/127.0.0.1/4222' 2>/dev/null; then
        echo "NATS server is ready on :4222"
        exit 0
    fi
    sleep 0.1
done

echo "ERROR: NATS server failed to start. Check /tmp/nats.log"
cat /tmp/nats.log
exit 1
