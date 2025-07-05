name: crond
desc: Daemon for executing scheduled commands

cmd: /sbin/crond

startup: base

restart: on-failure

tags: sys, cron
