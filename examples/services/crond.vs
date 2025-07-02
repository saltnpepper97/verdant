name: crond

cmd: /usr/sbin/crond
args: -f -d 8

startup-package: base

restart: on-failure
restart-delay: 3

tags: cron
