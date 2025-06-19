name = network-pre
description = Bring up loopback and eth0 interfaces
exec = /bin/sh
args = -c "ip link set lo up && ip link set eth0 up"
restart = never
