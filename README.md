# Breadbin466

Breadbin466 is a from-scratch Commodore 64 emulator written in Rust, built around a deliberately narrow idea: emulate one Commodore 64, and understand that machine well enough to model it rather than approximate it.

<p align="center"><img src="images/breadbin466-sonic.png" alt="Breadbin466 running Sonic the Hedgehog"></p>

The reference machine is a PAL Commodore 64 built around the Assy 250466 motherboard:

- MOS Technology 6510 CPU;
- MOS Technology 6569R5 VIC-II;
- MOS Technology 6581R4AR SID;
- two MOS Technology 6526A CIA chips;
- Commodore 1541 disk drive;
- Commodore 1530 Datassette;
- Commodore 1764 REU with 512 KB.

Breadbin466 does not try to be a catalogue of Commodore hardware revisions. There is no menu for choosing between many C64 models, VIC-II revisions or SID variants. The project instead concentrates its engineering effort on reproducing this single configuration consistently, including the interactions between its components.

That approach also applies to the source code. Breadbin466 is intended to remain readable as an engineering description of the machine. Timing, bus ownership, analogue behaviour, peripheral state and platform integration are kept explicit rather than hidden behind compatibility shortcuts.

The SID is a particularly important example. The current 6581 model is based on detailed characterisation work against a physical MOS 6581R4AR, datecode 2286, and models the oscillator, waveform and envelope DACs, combined waveforms, analogue mixing, volume DAC and nonlinear filter as parts of one signal path.

## Current scope

Breadbin466 currently includes:

- cycle-driven 6510 CPU and motherboard memory mapping;
- VIC-II raster timing, video generation and bus arbitration;
- analogue-oriented MOS 6581R4AR SID emulation;
- CIA timers, interrupts, TOD, serial logic, keyboard and joystick interfaces;
- IEC serial bus;
- cycle-driven Commodore 1541, including software fastloaders and protected disk behaviour;
- D64, G64, NIB and NBZ disk-image support, plus Breadbin466's open D7Z format;
- disk-image creation, conversion and space reclamation tools;
- Commodore 1530 Datassette;
- cartridge support, including EasyFlash and freezer cartridges;
- Commodore 1764 REU with 512 KB;
- Commodore 1351 proportional mouse support;
- an integrated debugger with breakpoints, disassembly, history and configurable execution/read/write tracing;
- command-line automation facilities for testing and characterisation;
- session-long PCM WAV audio capture;
- native desktop integration on macOS, Windows and Linux.

## D7Z disk images

Breadbin466 0.12 introduces D7Z, an open disk-image format proposed by the Breadbin466 project. D7Z is not a pre-existing Commodore or emulator format.

D7Z is a compact, lossless storage representation of exactly one D64 byte stream. It does not attempt to replace low-level formats such as G64, NIB or NBZ, and it does not preserve information that D64 itself cannot represent. A D7Z file therefore has exactly the same disk-image semantics as the D64 obtained by decompressing it.

Version 1.0 consists of a small, explicitly defined header followed by a raw LZMA2 stream. The header contains:

- an eight-byte D7Z signature;
- the D7Z major and minor format version;
- the header size;
- the LZMA2 dictionary size required for decoding;
- a SHA-256 digest of the complete uncompressed D64 byte stream.

The format deliberately stores no source filename, timestamp, creator, operating system, conversion history or filesystem-specific metadata. D64 geometry and the presence of an optional DOS error table are determined from the size of the decompressed D64 data, just as they are for the corresponding D64 image.

D7Z compression is semantically transparent. Creating a D7Z does not inspect or modify the BAM, clear unused sectors, normalise disk contents or otherwise alter the D64 byte stream. Breadbin466's separate Reclaim Space operation may be requested before conversion, but that transformation is not part of D7Z.

The format is designed to be independently implementable. A compliant encoder does not need to reproduce Breadbin466's exact compression parameters: different valid LZMA2 search parameters and dictionary sizes may produce different D7Z files representing the same D64. A decoder uses the dictionary size stored in the header and verifies the SHA-256 digest before accepting the reconstructed D64 image.

In short:

- **D64 and D7Z are logically equivalent.** D7Z is the compressed form.
- **G64 is a lower-level representation.** Converting arbitrary G64 data to D64 or D7Z may lose information that the logical formats cannot express.
- **NIB and NBZ are capture representations.** NBZ is the compressed counterpart of NIB; D7Z serves the analogous storage role for D64.
- **D7Z is a Breadbin466 format, but it is not intended to be Breadbin466-only.** Its on-disk structure is deliberately simple and documented so that other emulators and disk-image tools can implement it.

## Project status

Breadbin466 is under active development.

A wide range of games, demos, fastloaders, copy-protected disk images, cartridges and REU software already run. The emulator is nevertheless still treated as an engineering work in progress: when software exposes behaviour that disagrees with the reference hardware, the preferred solution is to understand the underlying machine behaviour rather than add a title-specific workaround.

<p align="center"><img src="images/breadbin466-basic.png" alt="Breadbin466 at the Commodore 64 BASIC screen"></p>

Bug reports are most useful when they include:

- the exact software version or image used;
- the steps required to reproduce the issue;
- the expected behaviour on real hardware, when known;
- relevant screenshots, debugger output or traces;
- whether the issue also occurs without optional hardware such as a cartridge or REU.

## Licence

Breadbin466 is licensed under the MIT License. See [`LICENSE`](LICENSE).

The Commodore ROM images distributed with Breadbin466 are not covered by the MIT License. See [`ROM-LICENSE`](ROM-LICENSE).

## Changelog

### 0.12.1

Version 0.12.1 focuses on hardware-accuracy fixes and introduces an explicit licence for Breadbin466.

- Breadbin466 is now released under the permissive **MIT License**. The Commodore ROM images distributed with Breadbin466 are not covered by the MIT License and retain their separate copyright status.
- Fixed numerous 6510 edge cases, including decimal arithmetic, interrupt/RDY timing, undocumented instructions and the 6510 I/O port.
- Improved CIA 6526 accuracy, including timers, interrupts, TOD clock and serial behaviour.
- Fixed several VIC-II timing and rendering edge cases and added light-pen emulation.
- Improved SID oscillator, noise, waveform and envelope behaviour based on hardware measurements.
- Improved 1541/VIA accuracy, including memory decoding, open-bus behaviour, mechanics, rotation, GCR and read-channel timing.
- Made C64/1541 IEC worker synchronisation more robust while retaining parallel drive execution and warp-mode performance.
- Improved REU DMA timing and register behaviour.
- Improved cartridge compatibility, including Retro Replay hardware variants.
- Added various smaller correctness and robustness fixes throughout the emulator.

### 0.12.0

Version 0.12.0 is primarily a hardware-fidelity and infrastructure release.

- Substantially revised the MOS 6581R4AR SID emulation following physical characterisation of a datecode 2286 chip. The analogue signal path now models waveform and envelope DAC behaviour, combined-waveform interaction, operating-point effects, mixer behaviour, D418 volume-DAC characteristics and the nonlinear filter in considerably greater detail.
- Reorganised the SID implementation into smaller components for control state, integration, mixing, response curves and transients, while keeping the model centred on the single 6581R4AR reference device.
- Added Commodore 1351 proportional mouse emulation on control port 1.
- Added a unified debugger trace recorder with configurable execute, read and write ranges, trace-file selection and explicit start, stop, flush, clear and status operations.
- Added session-long PCM WAV capture through the `--wav` command-line option, including silent capture without host audio output.
- Introduced D7Z 1.0, a new open disk-image format proposed by Breadbin466 for compact, lossless storage of D64 byte streams. D7Z uses a documented header, raw LZMA2 compression and SHA-256 verification, and is intended to be independently implementable by other emulators and disk-image tools.
- Added explicit disk-image space reclamation for conversion workflows, with format-aware handling intended to preserve representation-specific data where possible.
- Added native disk-image creation and conversion interfaces for macOS, Windows and Linux.
- Continued restructuring of the cycle-driven 1541 implementation, including clearer separation of image loading/saving, media handling, mechanics, read-channel behaviour and conversion utilities.
- Extended command-line and debugger facilities used for repeatable emulation, diagnostics and hardware-characterisation workloads.
- Continued cartridge, REU, CIA, VIC-II, motherboard and platform-integration corrections discovered through compatibility testing.

### 0.11.1

- Added the integrated debugger and supporting breakpoint, disassembly and execution-history infrastructure.
- Added Commodore 1764 REU emulation with 512 KB.
- Added REU controls and related command-line integration.
- Improved application and UI behaviour around media handling and emulator state.
- Included further compatibility and correctness fixes across the emulator.

### 0.11.0

Version 0.11.0 is the baseline for this changelog. Earlier development history is not reconstructed here.
