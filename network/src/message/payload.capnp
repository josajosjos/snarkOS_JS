@0x8ef0fbcbd0fb8d85; # unique file ID, generated by `capnp id`

struct Ipv4Addr {
    octets @0: List(UInt8);
}

struct SocketAddrV4 {
    addr @0 :Ipv4Addr;
    port @1 :UInt16;
}

struct Ipv6Addr {
    octets @0: List(UInt8);
}

struct SocketAddrV6 {
    addr @0 :Ipv6Addr;
    port @1 :UInt16;
    # flowInfo @2 :UInt32;
    # scopeId @3 :UInt32;
}

struct SocketAddr {
    addrType :union {
        v4 @0: SocketAddrV4;
        v6 @1: SocketAddrV6;
    }
}

struct BlockHash {
    hash @0 :Data;
}

struct Transaction {
    data @0 :Data;
}

struct Block {
    data @0 :Data;
}

struct Ping {
    blockHeight @0 :UInt32;
}

struct GetMemoryPool {
    placeholder @0 :Void;
}

struct GetPeers {
    placeholder @0 :Void;
}

struct Pong {
    placeholder @0 :Void;
}

struct Payload {
    payloadType :union {
        block @0 :Block;
        getBlocks @1 :List(BlockHash);
        getMemoryPool @2 :GetMemoryPool;
        getPeers @3 :GetPeers;
        getSync @4 :List(BlockHash);
        memoryPool @5 :List(Transaction);
        peers @6 :List(SocketAddr);
        ping @7 :Ping;
        pong @8 :Pong;
        sync @9 :List(BlockHash);
        syncBlock @10 :Block;
        transaction @11 :Transaction;
    }
}

struct Version {
    version @0 :UInt64;
    listeningPort @1 :UInt16;
    nodeId @2 :UInt64;
}
