name = crond
description = Cron scheduler
exec = /usr/sbin/crond
args = -f -l 0
restart = on-failure
