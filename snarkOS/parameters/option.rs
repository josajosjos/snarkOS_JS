use crate::parameters::types::*;

// Format
// (argument, conflicts, possible_values, requires)

// Global

pub const PATH: OptionType = (
    "[path] -d --path=[path] 'Specify the node's storage path'",
    &[],
    &[],
    &[],
);

pub const IP: OptionType = ("[ip] -i --ip=[ip] 'Specify the ip of your node'", &[], &[], &[]);

pub const PORT: OptionType = (
    "[port] -p --port=[port] 'Run the node on a specified port'",
    &[],
    &[],
    &[],
);

pub const RPC_PORT: OptionType = (
    "[rpc_port] --rpc_port=[rpc_port] 'Run the rpc server on a specified port'",
    &["no_jsonrpc"],
    &[],
    &[],
);

pub const CONNECT: OptionType = (
    "[connect] --connect=[ip] 'Specify a node ip address to connect to on startup'",
    &[],
    &[],
    &[],
);

pub const COINBASE_ADDRESS: OptionType = (
    "[coinbase_address] -c --coinbase_address=[coinbase_address] 'Run the node on a specified port'",
    &[],
    &[],
    &[],
);

pub const MEMPOOL_INTERVAL: OptionType = (
    "[mempool_interval] --mempool_interval=[mempool_interval] 'Specify the frequency in seconds x 10 the node should fetch the mempool from sync node'",
    &[],
    &[],
    &[],
);

pub const MIN_PEERS: OptionType = (
    "[min_peers] --min_peers=[min_peers] 'Specify the minimum number of peers the node should connect to'",
    &[],
    &[],
    &[],
);

pub const MAX_PEERS: OptionType = (
    "[max_peers] --max_peers=[max_peers] 'Specify the maximum number of peers the node can connect to'",
    &[],
    &[],
    &[],
);
