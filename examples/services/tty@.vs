name: tty@{id}
desc: Login prompt using getty on /dev/{id}

cmd: /sbin/getty
args: -L /dev/{id} 115200 linux

restart: always

tags: tty
