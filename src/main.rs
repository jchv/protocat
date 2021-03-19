extern crate nom;
extern crate nom_locate;

use std::env;
use std::fs::File;
use std::io::Read;
use std::ops::RangeFrom;

use nom::*;
use nom::combinator::*;
use nom::error::*;
use nom::number::complete::*;
use nom::bytes::complete::*;
use nom::multi::*;
use nom_locate::*;

type Span<'a> = LocatedSpan<&'a [u8]>;
type Error<I> = VerboseError<I>;

#[derive(Copy, Clone, Debug, PartialEq)]
enum WireType {
    VarInt,
    Int64,
    LengthPrefixed,
    StartGroup,
    EndGroup,
    Int32
}

#[derive(Copy, Clone, Debug, PartialEq)]
struct ProtoTag {
    wire_type: WireType,
    tag_number: u64,
}

#[derive(Copy, Clone, Debug, PartialEq)]
enum WireValue<I> {
    VarInt(u64),
    Int64(u64),
    LengthPrefixed(I),
    StartGroup,
    EndGroup,
    Int32(u32),
}

#[derive(Copy, Clone, Debug, PartialEq)]
struct ProtoField<I> {
    tag_number: u64,
    value: WireValue<I>,
}

type ProtoMessage<'a, I> = &'a [ProtoField<I>];

fn length_take<I, N, E, F>(mut f: F) -> impl FnMut(I) -> IResult<I, I, E>
where
    I: InputIter + InputTake + InputLength,
    N: ToUsize,
    F: Parser<I, N, E>,
    E: ParseError<I>,
{
    move |i: I| {
        let (i, count) = f.parse(i)?;
        take(count)(i)
    }
}

fn base128_vlq<'a, I, E>(input: I) -> IResult<I, u64, E>
where
    I: Clone + PartialEq + Slice<RangeFrom<usize>> + InputIter<Item = u8> + InputLength,
    E: ParseError<I>,
{
    let mut result = 0u64;
    let (input, (lead, tail)) = many_till(
        verify(le_u8, |c| c & 0x80 != 0),
        verify(le_u8, |c| c & 0x80 == 0),
    )(input)?;
    result |= (tail & 0x7f) as u64;
    for b in lead.as_bytes().iter().rev() {
        result <<= 7;
        result |= (*b & 0x7f) as u64;
    }
    Ok((input, result))
}

impl ProtoTag {
    fn parse<'a, I, E>(input: I) -> IResult<I, Self, E>
    where
        I: Copy + Clone + PartialEq + Slice<RangeFrom<usize>> + InputIter<Item = u8> + InputLength,
        E: ParseError<I> + ContextError<I>,
    {
        context("ProtoTag", move |input: I| -> IResult<I, Self, E> {
            let (input, tag) = base128_vlq(input)?;
            Ok((input, Self{
                wire_type: match tag & 0x7 {
                    0 => WireType::VarInt,
                    1 => WireType::Int64,
                    2 => WireType::LengthPrefixed,
                    3 => WireType::StartGroup,
                    4 => WireType::EndGroup,
                    5 => WireType::Int32,
                    _ => return Err(nom::Err::Failure(E::from_error_kind(input, ErrorKind::Verify)))
                },
                tag_number: tag >> 3,
            }))
        })(input)
    }
}

impl<I> ProtoField<I>
where
    I: Copy + PartialEq + Slice<RangeFrom<usize>> + InputIter<Item = u8> + InputLength + InputTake
{
    fn parse<E>(input: I) -> IResult<I, Self, E>
    where
        E: ParseError<I> + ContextError<I>
    {
        context("ProtoField", move |input: I| -> IResult<I, Self, E> {
            let (input, tag) = ProtoTag::parse(input)?;
            match tag {
                ProtoTag{wire_type: WireType::VarInt, tag_number} =>
                    map(base128_vlq, |value| ProtoField{tag_number, value: WireValue::VarInt(value)})(input),

                ProtoTag{wire_type: WireType::Int64, tag_number} =>
                    map(le_u64, |value| ProtoField{tag_number, value: WireValue::Int64(value)})(input),

                ProtoTag{wire_type: WireType::LengthPrefixed, tag_number} =>
                    map(length_take(base128_vlq), |value| ProtoField{tag_number, value: WireValue::LengthPrefixed(value)})(input),

                ProtoTag{wire_type: WireType::StartGroup, tag_number} =>
                    Ok((input, ProtoField{tag_number, value: WireValue::StartGroup})),

                ProtoTag{wire_type: WireType::EndGroup, tag_number} =>
                    Ok((input, ProtoField{tag_number, value: WireValue::EndGroup})),

                ProtoTag{wire_type: WireType::Int32, tag_number} =>
                    map(le_u32, |value| ProtoField{tag_number, value: WireValue::Int32(value)})(input)
            }
        })(input)
    }
}

fn protobuf<'a, I, E>(input: I) -> IResult<I, Vec<ProtoField<I>>, E>
where
    I: Copy + PartialEq + Slice<RangeFrom<usize>> + InputIter<Item = u8> + InputLength + InputTake,
    E: ParseError<I> + ContextError<I>
{
    many0(complete(ProtoField::parse))(input)
}

fn print_indent(indent: usize) {
    for _ in 0..indent {
        print!("  ")
    }
}

fn print_message(indent: usize, fields: ProtoMessage<LocatedSpan<&[u8]>>) {
    for field in fields.iter() {
        match field.value {
            WireValue::VarInt(v) => {
                print_indent(indent);
                println!("{}: {}", field.tag_number, v);
            }

            WireValue::Int64(v) => {
                print_indent(indent);
                println!("{}: {}", field.tag_number, v);
            }

            WireValue::LengthPrefixed(d) => {
                // Apply heuristics to attempt to drill deeper, going in order of most strict to least strict.
                // TODO: should probably handle certain cases (like all zeros should probably be raw data.)
                if let Ok((_, fields)) = all_consuming(protobuf::<_, Error<_>>)(d) {
                    // Treat as submessage.
                    print_indent(indent);
                    println!("{}: {{", field.tag_number);

                    print_message(indent + 1, &fields);

                    print_indent(indent);
                    println!("}}")
                } else if let Ok(str) = String::from_utf8(d.as_bytes().to_vec()) {
                    // Treat as string.
                    println!("{}: {}", field.tag_number, str);
                } else {
                    // Treat as raw data.
                    let data = d.as_bytes().to_vec();
                    println!("{}: {:x?}", field.tag_number, data);
                }
            }

            WireValue::StartGroup => {}
            WireValue::EndGroup => {}

            WireValue::Int32(v) => {
                print_indent(indent); println!("{}: {}", field.tag_number, v);
            }
        }
    }
}

fn main() {
    for name in env::args().skip(1) {
        // Read file.
        let mut f = File::open(name).expect("opening file failed");
        let mut buffer = Vec::new();
        f.read_to_end(&mut buffer).expect("reading file failed");

        // Parse, ensuring that we consume all bytes.
        let (_, fields) = all_consuming(protobuf::<_, Error<_>>)(
            Span::new(&buffer)
        ).expect("parse error");

        // Print message to stdout.
        print_message(0, &fields);
    }
}
