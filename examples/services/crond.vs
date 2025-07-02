name: crond

cmd: /usr/sbin/crond
args: -f

startup-package: base

restart: on-failure
restart-delay: 3

tags: cron
