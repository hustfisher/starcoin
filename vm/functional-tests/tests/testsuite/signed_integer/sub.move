script {
use 0x1::SignedInteger64;

fun main() {
    let i1 = SignedInteger64::create_from_raw_value(100, false);
    let zero = SignedInteger64::sub_u64(100, copy i1);
    assert(SignedInteger64::get_value(zero) == 0, 1);

    let negative = SignedInteger64::sub_u64(50, copy i1);
    assert(SignedInteger64::get_value(copy negative) == 50, 2);
    assert(SignedInteger64::is_negative(copy negative) == true, 3);

    let positive = SignedInteger64::sub_u64(150, copy i1);
    assert(SignedInteger64::get_value(copy positive) == 50, 4);
    assert(SignedInteger64::is_negative(copy positive) == false, 5);
}
}