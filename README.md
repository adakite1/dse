## Description

`dse` is a highly-compatible `.SWD` and `.SMD` file parser/writer based on Psy_Commando's [`ppmdu_2`](https://github.com/PsyCommando/ppmdu_2) parser, while also extending it with multi-levelled compat exporting and importing and thus, in many cases, binary-perfect imports and exports.

Work is on-going on this repo.

## Goal

**To be as compatible as possible while guaranteeing write validity**

This was written from the ground up to try to be as compatible as possible with the `.SWD` and `.SMD` file formats while also remaining as writable as possible. To achieve this, first all automatically generated parameters are written in on the fly based on specifications (things like `chunklen`), and secondly for the other values directly affecting the sound, decisions about the preservation of values of unknown meaning had to be made. There are currently 3 possible ways to use the parser:
- **`.SWD` => memory => `.SWD`**<br/>
Binary perfect recreation of the original file in most cases with slight differences in certain known byte values, not stripping any of the unknown bytes.
- **`.SWD` => memory => `.SWD.XML` => memory => `.SWD`**<br/>
Still perfect, but first converting the `.SWD` file into a readable `.SWD.XML` format to allow for editing.
- **`.SWD` => memory => `.SWD.XML` (with `serde_use_common_values_for_unknowns` set to `true`) => memory => `.SWD`**<br/>
The intended use case for most scenarios. In this mode, much of the unknown bytes are stripped during the serialization process to XML, leaving only the parameters we know the meaning of. The stripped values are then automatically filled in with the most common defaults during the deserialization process. All of this gives meaning to the tool, as it means that much of the unknown bytes *can be stripped with no issues!*
P.s. the XML file generated by this method is also the simplest

This also guarantees that, as long as the writer picks the appropriate level of verboseness they would like in their editing of these files, that whatever file comes out the other end of the pipe *should* be **absolutely correctly formatted** no matter what. (Please report it if it does not! Especially if you're turning the XML files into `.SWD` or `.SMD` files. That's probably a bug.)

**Important note about `.SMD` files:** All of the above is true for `.SMD` files as well, but there's a caveat. `.SMD` files contain `DSEEvents`, and particularly `PlayNote` events that allows for variable-length numbers as its parameters. The crate assumes that the original DSE engine compresses the `keydownduration` value to the least amount of bytes necessary to store the value, which means for example, that `0x0000000A` would just be stored as `0x0A`, and that `0x00000000` wouldn't even be stored, with the corresponding `NbParamBytes` set to zero. This is the pattern found in most `.SMD` files. **HOWEVER!** I've come across certain `.SMD` files that have stored a `0x00000000` as one byte `0x00`, even though that's completely unnecessary. The notable example is `bgm0016.smd`. In those cases there's just no way for the crate to properly generate the `NbParamBytes` to match the file in a binary-perfect way. While the crate *could* read the `.SMD` and just write it out again based on data in memory (because internally when the file is first read, not a single byte of the file is ever discarded), that'd just be a file copy in File Explorer! By deviating ever so slightly from the original file's formatting, it's possible to maintain writability, and to ensure that this is the only reason that things don't line up I've tested quite a bunch to see that the exported files from these problematic files also work perfectly fine in the original ROMs.

## Usage

While this can be used as a Rustlang crate for those who are feeling adventurous, the intended way to use it is to use the binaries the crate generates.

Build process:
1. Install Rustlang
2. `git clone https://github.com/adakite1/dse.git`
3. `cd dse`
4. `cargo build` or `cargo build --release` for the release optimized version

The binaries will be in `target/[debug or release]`.

## Examples

#### swdl_tool.exe
`.\swdl_tool.exe to-xml .\NDS_UNPACK\data\SOUND\BGM\*.swd -o unpack`<br/>
`.\swdl_tool.exe from-xml .\unpack\*.swd.xml -o .\NDS_UNPACK\data\SOUND\BGM\`<br/>
`.\swdl_tool.exe add-sf2 ./*.sf2 ./bgm.swd -t 20000 -S 20000 -l 6`

#### smdl_tool.exe
`.\smdl_tool.exe to-xml .\NDS_UNPACK\data\SOUND\BGM\*.smd -o unpack`<br/>
`.\smdl_tool.exe from-xml .\unpack\*.smd.xml -o .\NDS_UNPACK\data\SOUND\BGM\`<br/>
`.\smdl_tool.exe from-midi .\midi_export.mid ./bgm0043.swd --midi-prgch --generate-optimized-swdl`

**A quick note on the MIDI conversion functionality:**
- The MIDI file must be of type `smf0` or, in the case of `smf1`, be composed of 16 MIDI tracks or lower! (Not counting any meta event tracks at the very start if your music composition software exports those)
- Currently, the instruments are mapped automatically to the entries in the `prgi` chunk of the provided SWD file, channel number <=> index in the `prgi` chunk
- P.s., you can also just put in the `SWD.XML` file directly into that command and it should still work
- CC07 Volume, CC10 Pan Position, and CC11 Expression are the currently supported MIDI CC controllers; they're mapped to their SMDL equivalents
- If you want a track to loop and go back to an earlier position after reaching the end, you need to add a MIDI Meta event of the `Marker` type, and set it to "LoopStart" (this is in parity with `ppmdu_2` MIDI exports too!)
- ... That's all I can think of for now, if there's something else I'll update this. Ping me about any weirdness that happens!

**Important note:** DON'T RUN `dse.exe`!! It shouldn't do much but it's the file I use for doing very specific things like reading specifically named files and stuff for testing purposes. Use the other two :)

## Thanks to...
- `Psy_Commando` for the brilliant documentation of all the `.SWD` and `.SMD` files, and for the creation of the `ppmdu_2` parsers. It was invaluable in the creation of this parser, and laid the groundwork for my understanding of much of DSE's internal structures.
- `CodaHighland` from the Halley'sCometSoftware discord and `Psy_Commando` for the documentation of various new DSE events that, together, pretty much completely decipher the `SMDL` file format. While the absolute specifics of these events might need some more exploring, the fact that we know what all these events do has been invaluable to the parser, and also gives me a lot of hope that we can definitely make something that works reaallly well to convert MIDI.
- `nazberrypie`, creator of `Trezer`, for documenting the effects of the link bytes. This was invaluable in finally finishing the `.SMD` parser, and also helped me understand the linking structure of `.SMD` and `.SWD` files much better.
- The authors of the `byteorder`, `bevy_reflect`, `serde`, `quick-xml`, `base64`, `clap`, `glob`, `phf`, `midly`, `chrono`, `colored`, `soundfont`, `adpcm-xq`, and `r8brain` libraries for making them. It has helped me immensely as I attempted many times from various angles to get this to finally work.
- And finally, the `#reverse-engineering` channel on the SkyTemple discord! I know I'm somewhat new in the community, but `Adex`, `Nazberry`, `Psy_Commando`, `Mond`, and others have helped me so much in getting the information and boost I needed to make it this far. I honestly can't imagine finishing this so quickly if I had done it alone. Thanks guys!
