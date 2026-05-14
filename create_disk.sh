#!/bin/bash

DISK_IMAGE="build/disk.img"
DISK_SIZE_MB=32

echo "Creating disk image..."
dd if=/dev/zero of=$DISK_IMAGE bs=1M count=$DISK_SIZE_MB 2>/dev/null

echo "Formatting with ext2..."
mkfs.ext2 -F $DISK_IMAGE >/dev/null 2>&1

echo "Mounting disk image..."
MOUNT_POINT=$(mktemp -d)
sudo mount -o loop $DISK_IMAGE $MOUNT_POINT

echo "Creating test files..."
sudo mkdir -p $MOUNT_POINT/test
echo "Hello from Dunit OS!" | sudo tee $MOUNT_POINT/test/hello.txt >/dev/null
echo "Persistent storage works!" | sudo tee $MOUNT_POINT/test/persistent.txt >/dev/null
sudo mkdir -p $MOUNT_POINT/data

echo "Unmounting..."
sudo umount $MOUNT_POINT
rmdir $MOUNT_POINT

echo "Disk image created: $DISK_IMAGE"
