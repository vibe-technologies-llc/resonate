use crate::stun::binding_answer;

pub fn read_a_stun_answer(datagram: &[u8]) {
    let _ = binding_answer(datagram);
}
