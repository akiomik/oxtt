# Bela Gem Stereo Verification: hyperglare Under the Render Callback

What a Bela Gem Stereo did with `hyperglare-bela`, measured on the board. This
is the first milestone of the port: the resonator bank running under Bela's
callbacks with the chord and every setting from the command line. There is no
control surface, and that is not an accident — see
[ADR 0016](../../decisions/0016-the-bank-is-excited-per-band.md) for what the
DSP is and `crates/hyperglare-bela/src/app.rs` for why the panel is deferred.
**These runs predate the MIDI intake** and used a fixed chord throughout.

The CPU sweep is separate, in [`cpu.md`](cpu.md). This document is about
whether it sounds like itself.

For the setup these results come from, see
[`../../cross-compile.md`](../../cross-compile.md).

## Environment

| | |
| --- | --- |
| Board | Bela Gem Stereo on PocketBeagle 2, 4 cores |
| Image | Bela Debian Bookworm, 2026-03-25 |
| Kernel | `6.12.49-ti-arm64-r55-evl-2` |
| Binary | `hyperglare-bela`, release, cross-compiled per [`../../cross-compile.md`](../../cross-compile.md) |
| Enclosure | none — open board on a desk |
| Source | a player's line output into the board's input, at `--adc-gain-db 12` |
| Monitoring | RME Babyface Pro, board output into inputs 3/4, recorded on the host |
| Settings | 48 kHz, period 16, one render thread |

`bela_daemon` was stopped for every run. `--headphone-level-db -20` throughout,
which is well below the board's default and is what the recordings were taken
at.

## Results

### It starts, runs and stops — PASS

Every configuration in [`cpu.md`](cpu.md) started, ran to its timeout and
reported on exit. `SIGINT` ends a run cleanly and `cleanup` prints its
diagnostics.

### Audio passes through — PASS

At `--color 0` the board's output is the input. Recorded at the Babyface, the
octave-band profile of the board's dry path matches the source.

### `color` crossfades — PASS

Three runs on the same source, recorded and measured. Pitch-class
concentration in 500–2000 Hz, which is the measure
[ADR 0018](../../decisions/0018-hyperglare-is-judged-against-paired-recordings.md)
keeps for "is there a chord here":

```text
 --color        0.0     0.63     1.0
 chord        0.035    0.232   0.393
```

At zero it is the source, at one the source is inaudible and only the
resonators remain, and the default sits between. That is what
`docs/hyperglare/contracts.md` §5 says the knob does.

### The board and the offline renderer agree — PASS

**The result this document exists for.** The same settings, the same chord and
the same material, once through `hyperglare-render` on a development machine
and once through `hyperglare-bela` on the board:

```text
 octave bands, each row normalised to its own loudest
                        63    125    250    500     1k     2k     4k     8k    chord
  board   dry          0.0   -3.4  -10.0  -11.0   -9.9   -6.3   -3.7  -10.0    0.035
  offline dry          0.0   -1.8  -11.5  -14.3  -13.9  -10.0   -6.8  -13.3    0.028

  board   color 0.63   0.0   -5.7  -11.6  -13.9   -8.5   -6.8   -6.6  -14.3    0.232
  offline color 0.63   0.0   -5.1  -14.8  -18.5  -13.6  -11.5  -11.0  -18.6    0.248

  board   color 1.0    0.0   -7.4  -13.4  -16.7   -8.1   -7.0   -9.0  -33.1    0.393
  offline color 1.0    0.0   -7.7  -15.7  -19.4  -12.5  -11.0  -13.4  -38.8    0.396
```

Chord content matches to 0.016 and 0.003; the band profiles have the same
shape. The board's rows are a few decibels brighter across the middle because
the two are not the same audio — one is a file, the other is that file played
through a converter, a cable and the board's codec — but nothing the effect
does differs between them.

**The wet loses the top two bands at `--color 1.0`**, on both. That is the
mechanism rather than a fault: a resonator rings only on energy the source has
near it, so removing the dry removes the part of the high band the source was
carrying, and what is left is the grid, which stops at 5 kHz. The default mix
exists partly for this.

### Colour is audible, and how audible depends on the source — PASS, with a caveat

Judged by ear on the board, not from a recording.

- **Sparse, high, chordless material** — birdsong: colour is unmistakable. At
  `--color 1.0` the source is gone and the chord is all that is left.
- **A full mix** — a produced track: no colour was heard at any mix setting,
  and `--color 1.0` sounded muffled rather than chordal.

Both are the design working. A full mix has energy everywhere, so the same
resonance is masked; it also has the most energy at the bottom, so per-band
excitation drives the low resonators hardest, which is what "muffled" is. And
it already has a chord of its own, which a fixed grid argues with — the six
sources ADR 0018 judges against are all chordless, and the design this effect
comes from specifies bass and drums.

**So this is a limit worth knowing rather than a defect**: what
`contracts.md` §5.0 and `demo/hyperglare/README.md` say by measurement — the
source's reach is the effect's reach — is audible on the board.

### Keys reach the DSP — PASS

The MIDI intake (`--midi-port hw:0,0,0`, ADR 0021), played from a computer
over the board's USB gadget. `--report-on-exit` after each run, with the keys
in whatever state the run ended in:

```text
 what was sent                          held_voices  midi_backlog  underruns
 nothing                                          0             0          0
 3 note ons                                       3             3          0
 8 note ons, against a table of five              5             8          0
 3 note ons then 3 note offs                      0             3          0
 96 messages as fast as they would go             0            48          0
```

- **A played run starts empty.** No key, no chord — the fixed chord's default
  does not reach a run that takes keys.
- **The polyphony is the voice table.** Eight keys against five voices leaves
  five held; the rest stole and were stolen from.
- **The last row is the one worth having.** Forty-eight on/off pairs pushed
  through with no gap between them left every key up and nothing stuck, so no
  message was lost — `bela`'s ring holds 100 and reached 48. The drain's bound
  spread them over about twelve blocks and the callback met its deadline
  through all of it.

### The load does not move when a chord does — PASS

`active_resonators` read 35 in every run above — five voices of a seven-slot
block — whatever was held. CPU stayed between 12.8% and 13.9% across all five,
which is inside what the same configuration varies by between runs
([`cpu.md`](cpu.md)). **A chord arriving costs nothing**, which is what
reserving the blocks buys.

## Not verified

- **Latency.** Not measured. `oxtt` measured roughly 1 ms round trip on this
  board and nothing about this effect's structure should change that, but
  nothing about it has been checked either.
- **The noise floor.** `oxtt`'s is in
  [`../../oxtt/bela/noise-floor.md`](../../oxtt/bela/noise-floor.md) and is a
  property of the board's converters, so it applies here — but this effect's
  own contribution, with a bank of high-Q resonators ringing, has not been
  measured.
- **What MIDI sounds like.** The control path is verified above; the audio is
  not, and on this rig **the keys and the recording exclude each other**.
  Sending keys puts the board on the same machine as the capture interface, and
  the audio cable then closes a loop between three devices that are already
  bonded to each other. Neither device raises it alone — the sections above
  were captured through this same interface — and it is a property of the
  arrangement rather than of the mains, so the machine's power supply does not
  come into it.

  A key lift is meant to leave the voice ringing down at its own decay and a
  stolen voice is meant to start from rest. Both are measured offline
  (`crates/hyperglare-dsp/src/processor.rs`) and neither has been heard.
- **Anything with a control surface.** It does not exist.
- **Long runs.** The longest here was 40 seconds.
- **Chord changes while running.** `apply_params` retunes without a click by
  design, and on the board the chord is fixed for the run, so the path has
  never been exercised on hardware.

## Reproducing

```sh
export BELA_SYSROOT=/path/to/bela-sysroot
export BELA_PACKAGE=hyperglare-bela
scripts/bela-build.sh
scripts/bela-deploy.sh -- --report-on-exit --adc-gain-db 12 \
  --headphone-level-db -20
```

For the MIDI rows, `--midi-port hw:0,0,0` in place of `--notes`, and keys sent
from whatever the board's MIDI port is wired to. `amidi -l` names the port and
the argument takes the subdevice as well.

To record the board's output rather than listen to it, **hold the ssh session
open for the whole run**: a host started in the background and detached from
its session takes `SIGHUP` when ssh closes, and the recording then contains a
board that stopped playing partway through.

```sh
ssh root@bela.local "systemctl stop bela_daemon; \
  exec timeout --signal=INT 30 ./hyperglare-bela --adc-gain-db 12 \
  --headphone-level-db -20 --color 0.63 --notes 56,59,61,63,66" &
sleep 7   # the audio system takes a few seconds to come up
ffmpeg -f avfoundation -i ":<device>" -t 16 -c:a pcm_f32le out.wav
```

Check which channels of the capture device carry the board before trusting a
level: an aggregate device's channel numbering need not match the interface's
physical inputs.
