"""
Uniden Serial Keypad & Command Probe Utility
Tests raw key commands against connected scanners and inspects display/state changes.
"""
import sys
import time
import argparse
import serial

def probe_port(port: str, baud: int, test_keys: list):
    print(f"Opening {port} at {baud} baud...")
    try:
        ser = serial.Serial(port, baudrate=baud, timeout=0.5)
    except Exception as e:
        print(f"Error opening {port}: {e}")
        return

    time.sleep(0.5)
    ser.reset_input_buffer()

    # Initial probe
    ser.write(b"MDL\r")
    time.sleep(0.05)
    mdl = ser.read_all().decode("ascii", errors="replace").strip()
    print(f"Scanner Model Response: {repr(mdl)}")

    def send_cmd(cmd: str) -> str:
        ser.reset_input_buffer()
        ser.write((cmd + "\r").encode("ascii"))
        time.sleep(0.08)
        resp = ser.read_all().decode("ascii", errors="replace").strip()
        return resp

    print("-" * 65)
    print(f"{'Command Sent':<18} | {'Immediate Reply':<15} | {'STS Snapshot / Notes'}")
    print("-" * 65)

    for key in test_keys:
        # Pre-poll STS
        pre_sts = send_cmd("STS")
        # Send Key
        key_reply = send_cmd(key)
        # Post-poll STS
        time.sleep(0.1)
        post_sts = send_cmd("STS")

        diff = "No change"
        if pre_sts != post_sts:
            diff = f"Display state changed! Length: {len(pre_sts)} -> {len(post_sts)}"

        print(f"{key:<18} | {repr(key_reply):<15} | {diff}")
        time.sleep(0.3)

    ser.close()
    print("-" * 65)
    print("Probe complete.")

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Probe Uniden scanner keypad commands.")
    parser.add_argument("--port", default="COM6", help="COM port (e.g. COM5 or COM6)")
    parser.add_argument("--baud", type=int, default=19200, help="Baud rate (19200 for 436, 57600/115200 for 996)")
    parser.add_argument("--keys", nargs="*", default=[
        "KEY,>,P", "KEY,<,P", "KEY,^,P",
        "KEY,V,P", "KEY,V,L",
        "KEY,A,L", "KEY,B,L", "KEY,C,L",
        "KEY,S,P", "KEY,H,P", "KEY,M,P"
    ], help="List of KEY commands to test")
    args = parser.parse_args()

    probe_port(args.port, args.baud, args.keys)
