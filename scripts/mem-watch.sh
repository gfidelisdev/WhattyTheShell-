#!/usr/bin/env bash
# Usage: scripts/mem-watch.sh
# Monitora o processo whatty enquanto você usa o terminal
PID=$(pgrep -n -x whatty) || { echo "whatty não está rodando. Abra o WhattyTheShell primeiro."; exit 1; }
echo "Monitorando whatty (PID $PID)... Pressione Ctrl+C para parar."
printf "%-10s %-12s %-10s %-8s %-10s\n" "HORA" "VmRSS" "THREADS" "FDS" "PROCESSOS_FILHOS"
while kill -0 "$PID" 2>/dev/null; do
  rss=$(awk '/VmRSS/{print $2 " " $3}' /proc/$PID/status)
  thr=$(awk '/Threads/{print $2}' /proc/$PID/status)
  fds=$(ls /proc/$PID/fd 2>/dev/null | wc -l)
  kids=$(pgrep -P "$PID" 2>/dev/null | wc -l)
  printf "%-10s %-12s %-10s %-8s %-10s\n" "$(date +%T)" "$rss" "$thr" "$fds" "$kids"
  sleep 1
done
echo "whatty finalizado."
