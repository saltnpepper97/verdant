name: udhcpc@{}
desc: Lightweight DHCP client

cmd: /sbin/udhcpc
args: -i {} -q

startup: network

restart: on-failure

instances:
    - eth0

tags: net, dhcp
