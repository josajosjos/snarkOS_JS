#!/bin/bash

echo "Enter your miner address:";
read MINER_ADDRESS

if [ "${MINER_ADDRESS}" == "" ]
then
    echo "You need a miner address."
    exit 1
fi

COMMAND="cargo run --release -- --prover ${MINER_ADDRESS} --pool 69.10.36.174:4132 --verbosity 1"

for word in $*;
do
  COMMAND="${COMMAND} ${word}"
done

function exit_node()
{
    echo "Exiting..."
    exit
}

trap exit_node SIGINT

while :
do
  echo "Running the node..."

  $COMMAND

  sleep 2;
done
