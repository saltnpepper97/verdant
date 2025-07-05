name: syslogd
desc: System wide logging daemon

cmd: /sbin/syslogd

startup: system

restart: on-failure

tags: sys, log
