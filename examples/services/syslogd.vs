name: syslogd
desc: System logger daemon

cmd: /sbin/syslogd

startup-package: base

restart: on-failure
restart-delay: 1

tags: sys, log
