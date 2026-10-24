extern crate std;

use super::*;
use smoltcp::phy::RxToken;
use std::{collections::VecDeque, sync::Mutex, vec, vec::Vec};
use usb_device::{
    bus::PollResult,
    endpoint::{EndpointAddress, EndpointType},
    prelude::*,
    UsbDirection,
};

struct Bus {
    packets: Mutex<VecDeque<Result<Vec<u8>>>>,
}

impl UsbBus for Bus {
    fn alloc_ep(
        &mut self,
        direction: UsbDirection,
        address: Option<EndpointAddress>,
        _: EndpointType,
        _: u16,
        _: u8,
    ) -> Result<EndpointAddress> {
        Ok(address.unwrap_or_else(|| EndpointAddress::from_parts(1, direction)))
    }
    fn enable(&mut self) {}
    fn reset(&self) {}
    fn set_device_address(&self, _: u8) {}
    fn write(&self, _: EndpointAddress, bytes: &[u8]) -> Result<usize> {
        Ok(bytes.len())
    }
    fn read(&self, _: EndpointAddress, bytes: &mut [u8]) -> Result<usize> {
        let packet = self
            .packets
            .lock()
            .unwrap()
            .pop_front()
            .ok_or(UsbError::WouldBlock)??;
        if packet.len() > bytes.len() {
            return Err(UsbError::BufferOverflow);
        }
        bytes[..packet.len()].copy_from_slice(&packet);
        Ok(packet.len())
    }
    fn set_stalled(&self, _: EndpointAddress, _: bool) {}
    fn is_stalled(&self, _: EndpointAddress) -> bool {
        false
    }
    fn suspend(&self) {}
    fn resume(&self) {}
    fn poll(&self) -> PollResult {
        PollResult::None
    }
}

fn ntb(payload: [u8; 4]) -> Vec<u8> {
    let mut bytes = vec![
        b'N', b'C', b'M', b'H', 12, 0, 0, 0, 32, 0, 12, 0, b'N', b'C', b'M', b'0', 16, 0, 0, 0, 28,
        0, 4, 0, 0, 0, 0, 0,
    ];
    bytes.extend_from_slice(&payload);
    bytes
}

fn consumed_frame_survives_next_error(next: Result<Vec<u8>>) {
    let alloc = UsbBusAllocator::new(Bus {
        packets: Mutex::new(VecDeque::from([Ok(ntb(*b"good")), next, Ok(ntb(*b"next"))])),
    });
    let endpoint = alloc.bulk(64);
    let _device = UsbDeviceBuilder::new(&alloc, UsbVidPid(0x1209, 0x0002)).build();
    let mut store = [0; NTB_MAX_SIZE_USIZE];
    let mut out = OutBuf {
        read_ep: endpoint,
        buffer: RWBuffer::new(&mut store),
        datagram_len: None,
    };
    assert!(out.can_read());
    let mut consumed = 0;
    let value = EthernetRxToken::new(&mut out).consume(|bytes| {
        assert_eq!(bytes, b"good");
        consumed += 1;
        123
    });
    assert_eq!(value, 123);
    assert_eq!(consumed, 1);
    assert!(!out.can_read());
    assert!(out.can_read());
    EthernetRxToken::new(&mut out).consume(|bytes| assert_eq!(bytes, b"next"));
}

#[test]
fn malformed_next_ntb_does_not_panic_after_delivery() {
    consumed_frame_survives_next_error(Ok(vec![0; 12]));
}

#[test]
fn next_usb_read_error_does_not_panic_after_delivery() {
    consumed_frame_survives_next_error(Err(UsbError::InvalidState));
}
