#!/usr/bin/env python3
import struct
import math
import sys
import os
import time

# CRC-16/CCITT calculation (Init: 0xFFFF, Poly: 0x1021)
def compute_crc16(data: bytes) -> int:
    crc = 0xFFFF
    for byte in data:
        crc ^= (byte << 8)
        for _ in range(8):
            if crc & 0x8000:
                crc = ((crc << 1) ^ 0x1021) & 0xFFFF
            else:
                crc = (crc << 1) & 0xFFFF
    return crc

# Helper to pack CCSDS primary header
# Bytes 0-1: Version (3 bits) | Type (1 bit) | Sec Hdr Flag (1 bit) | APID (11 bits)
# Bytes 2-3: Seq Flags (2 bits) | Seq Count (14 bits)
# Bytes 4-5: Packet Data Length (16 bits) = data_field_len - 1
def make_ccsds_header(apid: int, seq_count: int, data_len: int, version: int = 0, has_sec_hdr: bool = True) -> bytes:
    b0_1 = ((version & 0x07) << 13) | (0 << 12) | ((1 if has_sec_hdr else 0) << 11) | (apid & 0x07FF)
    b2_3 = (3 << 14) | (seq_count & 0x3FFF)  # Standalone flag (3)
    b4_5 = (data_len - 1) & 0xFFFF
    return struct.pack(">HHH", b0_1, b2_3, b4_5)

# Generate parameters for APID 42 (Propulsion Module)
# Volt (sine), Temp (cosine), Status (healthy=170, fail=0)
def generate_apid42_payload(index: int) -> bytes:
    # We want to vary Volt and Temp over time to simulate telemetry
    volt_raw = int(120 + 20 * math.sin(index / 100.0)) & 0xFF # around 120-140 (calibrated: 60-70V)
    temp_raw = int(30 + 10 * math.cos(index / 150.0)) & 0xFF  # around 20-40 (calibrated: 10-20C)
    status_raw = 170 if (index % 100 != 0) else 0             # 99% healthy, 1% fail
    return struct.pack("BBB", volt_raw, temp_raw, status_raw)

# Generate other APID payload (just random bytes)
def generate_other_payload(apid: int, index: int) -> bytes:
    return struct.pack("BBB", (apid + index) & 0xFF, (index * 2) & 0xFF, 0xAA)

def generate_packet(apid: int, seq_count: int, timestamp_ns: int, index: int,
                    corrupt_crc: bool = False,
                    malformed_version: bool = False,
                    missing_sec_hdr: bool = False,
                    wrong_length_val: int = None) -> bytes:
    # 1. User Payload
    if apid == 42:
        payload = generate_apid42_payload(index)
    else:
        payload = generate_other_payload(apid, index)

    # 2. Secondary Header (8-byte timestamp)
    sec_hdr = struct.pack(">Q", timestamp_ns)

    # 3. Data Field (Secondary Header + Payload + 2-byte CRC space)
    # Total data field size without CRC bytes = 8 + len(payload)
    # Total data field size with CRC bytes = 8 + len(payload) + 2
    # So len_field = 10 + len(payload)
    data_field_len = 8 + len(payload) + 2
    
    # Primary header
    header = make_ccsds_header(
        apid=apid,
        seq_count=seq_count,
        data_len=data_field_len if wrong_length_val is None else wrong_length_val,
        version=1 if malformed_version else 0,
        has_sec_hdr=False if missing_sec_hdr else True
    )

    # Assemble packet for CRC computation
    packet_pre_crc = header + sec_hdr + payload
    
    # Compute CRC
    crc = compute_crc16(packet_pre_crc)
    if corrupt_crc:
        crc ^= 0xFFFF # Flip all bits of the CRC

    return packet_pre_crc + struct.pack(">H", crc)

def generate_dataset(output_dir: str, name: str, format_type: str, num_packets: int, fault_profile: str = None):
    ext = "ccsds" if format_type == "ccsds" else "bin"
    file_path = os.path.join(output_dir, f"{name}.{ext}")
    print(f"Generating {file_path} with {num_packets} packets (Fault profile: {fault_profile})...")
    
    # We want to simulate the 5 APIDs: 42, 43, 44 (sat 101), 50, 51 (sat 102)
    # We will interleave them with realistic timestamps starting from now
    apids = [42, 43, 44, 50, 51]
    # Sequence counters per APID
    seq_counters = {apid: 0 for apid in apids}
    
    # Timestamp starts at current time in nanoseconds
    start_time_ns = int(time.time() * 1_000_000_000)
    
    # Open file for writing
    with open(file_path, "wb") as f:
        for idx in range(num_packets):
            # Interleave APIDs based on index
            # Rates: APID 42 (50%), APID 43 (20%), APID 44 (10%), APID 50 (15%), APID 51 (5%)
            r = idx % 100
            if r < 50:
                apid = 42
            elif r < 70:
                apid = 43
            elif r < 80:
                apid = 44
            elif r < 95:
                apid = 50
            else:
                apid = 51
                
            # Time increment: average 10ms per packet (100 Hz aggregate)
            # We can simulate burstiness: sometimes 2ms, sometimes 20ms
            time_inc_ns = 10_000_000 # 10ms
            if fault_profile == "burst" and (idx % 100 < 20):
                time_inc_ns = 500_000 # 0.5ms (burst)
            elif fault_profile == "burst" and (idx % 100 >= 80):
                time_inc_ns = 50_000_000 # 50ms (idle)
                
            timestamp_ns = start_time_ns + idx * time_inc_ns
            
            # Apply fault injections if requested
            corrupt_crc = False
            malformed_version = False
            missing_sec_hdr = False
            duplicate_packet = False
            target_apid = apid
            
            if fault_profile == "corrupted":
                # Inject downstream pipeline failures at 2% rate each
                f_rand = idx % 100
                if f_rand == 0:
                    corrupt_crc = True
                elif f_rand == 1:
                    malformed_version = True
                elif f_rand == 2:
                    # Sequence gap
                    seq_counters[apid] = (seq_counters.get(apid, 0) + 5) % 16384
                elif f_rand == 3:
                    # Duplicate packet
                    duplicate_packet = True
                elif f_rand == 4:
                    # Unidentified APID
                    target_apid = 99
                    
            seq_count = seq_counters.get(target_apid, 0)
            
            # Generate CCSDS packet
            pkt = generate_packet(
                apid=target_apid,
                seq_count=seq_count,
                timestamp_ns=timestamp_ns,
                index=idx,
                corrupt_crc=corrupt_crc,
                malformed_version=malformed_version,
                missing_sec_hdr=False,
                wrong_length_val=None
            )
            
            # Advance sequence counter
            if not duplicate_packet:
                seq_counters[target_apid] = (seq_counters.get(target_apid, 0) + 1) % 16384
            
            # If wrapped binary format, wrap in sync header
            # Binary frame layout: sync(4) + length(2) + timestamp(8) + payload(N)
            if format_type == "binary":
                sync_word = b"\x1A\x2B\x3C\x4D"
                bin_hdr = sync_word + struct.pack(">HQ", len(pkt), timestamp_ns)
                frame = bin_hdr + pkt
            else:
                frame = pkt
                
            # Write to file
            f.write(frame)
            
            # Write duplicate if requested
            if duplicate_packet:
                f.write(frame)
                
    print(f"Dataset {name} generated successfully. Size: {os.path.getsize(file_path)} bytes.")

if __name__ == "__main__":
    if len(sys.argv) < 5:
        print("Usage: dataset_generator.py <output_dir> <name> <format: ccsds|binary> <num_packets> [fault_profile]")
        sys.exit(1)
        
    out_dir = sys.argv[1]
    name = sys.argv[2]
    fmt = sys.argv[3]
    count = int(sys.argv[4])
    profile = sys.argv[5] if len(sys.argv) > 5 else None
    
    os.makedirs(out_dir, exist_ok=True)
    generate_dataset(out_dir, name, fmt, count, profile)
