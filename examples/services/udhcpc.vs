name = udhcpc
description = DHCP client for eth0
exec = /sbin/udhcpc
args = -i eth0
restart = on-failure
