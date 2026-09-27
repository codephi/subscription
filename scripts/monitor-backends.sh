#!/bin/sh

interval=${MONITOR_INTERVAL:-2}

while :; do
  printf '\033[2J\033[H'
  printf 'Uso dos backends Rust (atualiza a cada %s s; Ctrl+C para sair)\n\n' "$interval"
  printf '%-14s %-8s %-12s %-10s %s\n' 'Backend' 'PID' 'Memória RSS' 'CPU' 'Processo'
  ps -axo pid=,rss=,%cpu=,args= | awk '
    function show_backend(name, pid, rss, cpu, command) {
      printf "%-14s %-8s %8.1f MiB   %7.1f%%   %s\n", name, pid, rss / 1024, cpu, command
    }
    {
      command = $4
      for (field = 5; field <= NF; field++) command = command " " $field
      if (command ~ /(^|\/)subscription([[:space:]]|$)/) {
        show_backend("Subscription", $1, $2, $3, command)
        subscription_found = 1
      } else if (command ~ /(^|\/)tasklab([[:space:]]|$)/) {
        show_backend("TaskLab", $1, $2, $3, command)
        tasklab_found = 1
      }
    }
    END {
      if (!subscription_found) print "Subscription   não está rodando"
      if (!tasklab_found) print "TaskLab        não está rodando"
    }
  '
  sleep "$interval"
done
