name = udhcpc
description = DHCP client for eth0
exec = /sbin/udhcpc
args = -i eth0 -f
restart = on-failure
requires = eth0-up
