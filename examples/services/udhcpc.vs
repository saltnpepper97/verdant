name: udhcpc
desc: lighweight DHCP Client

cmd: /sbin/udhcpc
args: -i eth0 -q

startup-package: network

restart: on-failure
restart-delay: 3

tags: dhcp, net
