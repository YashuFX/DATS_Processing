pub mod common {
    pub mod v1 {
        tonic::include_proto!("must.common.v1");
    }
}

pub mod telemetry {
    pub mod v1 {
        tonic::include_proto!("must.telemetry.v1");
    }
}

pub mod events {
    pub mod v1 {
        tonic::include_proto!("must.events.v1");
    }
}

pub mod replay {
    pub mod v1 {
        tonic::include_proto!("must.replay.v1");
    }
}

pub mod gateway {
    pub mod v1 {
        tonic::include_proto!("must.gateway.v1");
    }
}
