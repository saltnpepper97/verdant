name: tty@{id}
desc: Login prompt using getty on /dev/{id}

cmd: /sbin/getty
args: -L 115200 /dev/{id} linux

restart: always

tags: tty
