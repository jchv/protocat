# protocat
protocat is a quick app for dumping raw protocol buffers data. It does not currently support parsing protobuf schema files, although it might happen in the future. Currently, this is just intended to be useful for quick and dirty analysis of binary blobs.

For more information on the protocol buffers wire format, see the [Encoding | Protocol Buffers](https://developers.google.com/protocol-buffers/docs/encoding) page on the official protocol buffers documentation.

## Heuristics
The wire format for protocol buffers is very minimalist; therefore, we only know the bare minimum type information to continue parsing the protocol buffers, but we cannot ascertain their meaning. Constructs like oneof, maps, and submessages basically don't exist on the wire format level. In order to make this tool usable, it applies some very minimal heuristics:

 *  All integers are treated as unsigned. There is no attempt to try to detect or decode zigzag-encoded signed integers. They are not really that common and there's no obvious way to distinguish them.

 *  Length-prefixed data is handled heuristically by trying to parse it several different ways, starting from the most strict possibilities going to the least strict.

    First it will try to run a parser for a submessage, and if that succeeds, the length prefixed data will be treated as a submessage. This has some caveats; some strings can accidentally end up being valid protobuf. Additionally, evenly sized arrays of all zeros are treated as protocol buffers.

    Next it will try to parse the data as UTF-8. This is a bit less likely to succeed on accident for arbitrary data, especially if we've already ruled out a submessage.
  
    Finally, it will treat the data as raw data and print it in hexadecimal form.

## Output Format
The output format is very simple; it looks like this:

```
2: {
  1: 804273995
  2: {
    1: 804273995
  }
  2: {
    1: 804274000
    2: {
      1: 3
      2: 804274249
    }
  }
  3: 804274249
  3: 804274006
  4: 0
}
```

Because we're dealing with raw protocol buffers, tag names are not known; you will instead see tag numbers. (This may be rectified in the future, if you have the proto schema.) Meanwhile, integer values are assumed to be unsigned and displayed as decimal numbers. (If you happen to come across signed integers, you will currently need to manually zigzag decode them.)
