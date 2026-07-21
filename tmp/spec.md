# oxtt プロジェクトノート（スコープ・ハードウェア・ロードマップ）

## この文書について

旧 `tmp/spec.md`（DSPアーキテクチャ・契約・設計判断を含む単一の仕様書）は、技術文書に該当する内容を `docs/`（`architecture.md`、`contracts.md`、`decisions/`、`development.md`）へ移設したうえで削除した。本書は、その移設時に「概要・目標・スコープ・バックログ・ロードマップは技術文書ではない」という方針のもとdocsへは移さなかった内容——プロジェクトの位置づけ、スコープ、対象ハードウェア、物理コントロール設計、ロードマップ、将来拡張——を復元し、作業用ノートとして保持するものである。

`tmp/` は `.gitignore` 対象のスクラッチ領域であり、README・docs のように正式な成果物ではない。DSPの信号フロー・契約・設計判断そのものは `docs/` を参照すること。

MUST/SHOULD/MAY の表記は、それぞれ必須、原則として実施、任意を表す（旧仕様書の用語をそのまま踏襲）。

## 1. プロジェクトの位置づけ

最終的には Raspberry Pi 5 とUSBオーディオインターフェースを組み合わせ、机上で演奏に使えるDIYハードウェアエフェクターにする。

開発はPC上のDSP検証から段階的に進める。Raspberry Piへの組み込み・常駐化は任意の最終段階とし、物理スイッチとポテンショメータを接続して演奏できる段階を、本プロジェクトの実用上の到達点とする。

本実装は Xfer OTT や参照実装とのバイナリ互換、プリセット互換、サンプル単位の出力一致を目標にしない。製品名、UI、コード、アセットも複製せず、公開されている一般的な DSP 技法から独立実装する（README にも記載）。

## 2. OTTの定義（v0.1）

v0.1 は、OTTらしい主要因を「3バンド分割」「下側の持ち上げ」「上側の抑制」「帯域ごとに異なる時間応答」と定義する。次を独自の安全策として採用する。

- ステレオチャンネル間で検出信号をリンクし、定位の揺れを防ぐ（`docs/decisions/0002`）。
- クロスオーバー済みの dry 信号と処理済み信号を混ぜ、位相差によるコムフィルターを防ぐ（`docs/decisions/0001`, `0004`）。
- 最終リミッターやサチュレーターをコア仕様に含めず、コンプレッサー単体の挙動を検証可能にする。
- オーディオコールバックからヒープ確保、ロック、標準入出力を排除する（`docs/contracts.md` §6）。

## 3. スコープ

### 3.1 v0.1 の目標

- まずJACKが利用可能なPC上で動くステレオ・オーディオエフェクトを提供する。
- 3バンドそれぞれにアップワード／ダウンワード・コンプレッションを適用する。
- OTT風の初期値で、起動直後から効果を確認できる。
- コア DSP を JACK から分離し、単体テストとオフラインテストを可能にする。
- 任意の JACK バッファサイズをリアルタイム安全に処理する。
- 同じDSPコアを、後続段階でRaspberry Pi 5へ変更なく移植できる構造にする。

### 3.2 v0.1 の非目標

- VST3、CLAP、Audio Unit などのプラグイン形式
- GUI、波形表示、ゲインリダクションメーター
- 外部サイドチェイン
- look-ahead、オーバーサンプリング、サチュレーション、エキサイター、最終リミッター
- Xfer OTT のプリセット読み込みや完全な音響エミュレーション
- PC検証段階でのGUI、DAWオートメーション、sample-accurate automation
- 5.1ch など2chを超えるチャンネル構成
- 販売、量産、法規制への適合、第三者向けの製品サポート
- 電源断やプロセス停止時にも音を通すハードウェアtrue bypass
- 起動時間、可用性、障害復旧時間に関する製品グレードの保証

実行中の制御を非目標とするのはPC検証段階だけである。DSP API 自体は、後続のGPIOスイッチやポテンショメータから安全にパラメータを更新できる形にする。

### 3.3 対象ハードウェア

| 役割 | 初期構成 |
| --- | --- |
| SBC | Raspberry Pi 5 |
| OS | Raspberry Pi OS Lite 64-bit（初期検証ではTrixieの固定image） |
| 冷却 | Active Coolerまたは同等の能動冷却を推奨 |
| Raspberry Pi電源 | 公式27 W USB-C電源または同等の5 V / 5 A電源を推奨 |
| audio I/O | RME Babyface Pro FS、Class Compliant mode |
| 接続 | BabyfaceをRaspberry PiのUSBポートへ直接接続。USB hubは原則使わない |
| sample rate | 48 kHzを基準とする |
| audio channels | stereo capture / stereo playback |

初期段階ではaudio HATを使用しない。将来I2S HATへ変更する場合も、ALSA/JACKから2ch full-duplexデバイスとして利用できる限り、DSPコアを変更しない。

### 3.4 技術スタック

| レイヤー | 選択 | 方針 |
| --- | --- | --- |
| language | Rust 2024 Edition（rustc 1.85以降） | `aarch64-unknown-linux-gnu` をRaspberry Pi用targetとする |
| DSP | 独自Rustモジュール、`f32` | host APIに依存させない（`docs/decisions/0007`） |
| PC audio host | JACKまたはPipeWire JACK互換 | 仮説検証と計測に使う |
| Raspberry Pi audio host | JACK2 + ALSA（baseline）、必要時のみALSA direct | Babyfaceを単一のfull-duplexデバイスとして開く |
| Rust binding | 現行は`jack` crate 0.13系、native backendは`alsa` crate | CPALは導入しない（`docs/decisions/0007`） |
| controls | GPIO + 外付けADC | audio callbackとは別threadで読み取る |
| service manager | systemd | 任意の常駐化段階で導入する |

JACKは初期・DIY用途に十分である。ただし `OttProcessor` とJACK adapterの分離を維持し、将来必要になった場合だけALSA direct backendを追加できる構造にする（`docs/decisions/0007`）。低レベルaudio I/Oのクロスプラットフォーム共通化は目的ではないため、Raspberry Pi native backendにはCPALを導入せず、`alsa` crateを使ってcapture/playbackを同一RT loopで処理する。Raspberry Pi実機で要求レイテンシーと安定性を満たす限り、JACKからALSA directへ移行しない。

## 4. 対応フォーマット（外部仕様の要点）

| 項目 | 仕様 |
| --- | --- |
| チャンネル | stereo in / stereo out |
| サンプル型 | JACK native `f32` |
| 基準 sample rate | 48 kHz |
| DSP検証対象 sample rate | 44.1、48、96、192 kHz |
| buffer size | 1以上、JACKが通知する任意のフレーム数 |
| DSPアルゴリズムの追加レイテンシー | 0 sample |
| tail | なし。ただし停止・再開時に内部状態をリセットする |

片側だけが接続されている場合、そのポートは JACK の無音入力として扱う。片側をもう一方へ暗黙に複製しない。

JACK、ALSA、USB転送、BabyfaceのADC/DACによるround-trip latencyは0ではない。Raspberry Pi段階で実測し、DSPアルゴリズムのレイテンシーと区別して記録する。

## 5. RME Babyface Pro FS

本書では、使用機器を正式名称の `RME Babyface Pro FS`（以下Babyface）と表記する。Raspberry PiではClass Compliant mode（CC mode）で使用する。

RME公式マニュアルで確認できるCC modeの性質:

- USB Audio Class 2.0としてLinuxから標準認識でき、RME専用driverを必要としない。
- 最大24 bit / 192 kHz、複数入出力を公開できる。
- RME専用driver使用時と異なり、TotalMix FXと内蔵effectsは利用できない。
- `SELECT` と `DIM` を長押しし、level meterに `CC` が表示されるとCC modeになる。
- analog output 1/2が基本playback先である。現行の実機検証では、Line 3/4を入力し、Phones 3/4へ出力する経路を採用する。

初期接続仕様:

- BabyfaceをUSB hubを介さずRaspberry Piへ接続する。
- 入出力を別々のaudio deviceに分けず、Babyface 1台をcapture/playbackのclock masterとして使う。
- JACKは48 kHzでBabyfaceを開く。
- 初期検証ではSPDIF/ADATへ外部digital clockを入力しない。使用する場合はJACKと外部clockのsample rateを一致させる。
- `jack_lsp` で実際のsystem port名とchannel順を確認してから接続する。`system:capture_1` 等の名前をコードへ固定しない。
- 基準経路は、現行の実機検証に合わせてBabyfaceのLine 3/4から `oxtt:input_l/r`、`oxtt:output_l/r`からPhones 3/4とする。XLR Mic/Line 1/2やMain output 1/2を使う場合は、起動scriptのmappingだけを変更する。

BabyfaceはPiからのUSB bus powerで動作する可能性があるが、phantom power使用時などの瞬間的な電力変動を考慮し、RME互換の外部電源（9〜12 V、約1 A）を推奨する。少なくともRaspberry Pi側には5 V / 5 A級の安定した電源を使い、低電圧・USB切断が起きないことを確認する。

Raspberry Piへ接続する前に、必要に応じてRMEが対応するPC環境でBabyfaceのfirmwareを更新する。Pi上ではTotalMixへ依存せず、input gainとoutput levelはBabyface本体または本プロジェクトのcontrolsで設定する。

### 5.1 将来のI2S audio HAT移行

audio HATは段階2でBabyfaceのfull-duplex経路を検証した後に導入する。最初からHATに依存せず、Babyfaceで確認したJACK/DSPの挙動を基準にしてHAT側のALSA/JACKデバイスを比較する。HATへ移行しても、`OttProcessor`、control thread、parameter snapshot、物理controlの実装は変更しない。

第一候補は `HiFiBerry DAC+ ADC` とする。stereo input / stereo output、24 bit / 192 kHz、ライン入力を備え、今回の2ch full-duplex要件に直接対応するためである。ただし国内販売在庫が安定しない場合があるため、購入時に国内在庫を確認し、輸入を許容できる場合に選択する。

国内での入手性と公式サポートを優先する代替候補は `Raspberry Pi Codec Zero` とする。40ピンGPIOのPiに対応し、AUX IN / AUX OUT経由でstereo input / outputを利用できる。ただし最大96 kHzで、AUX入出力用のコネクタや配線を別途用意する必要がある。内蔵マイクとモノラルスピーカー機能は本プロジェクトでは使用しない。

HAT移行時の購入候補:

- `HiFiBerry DAC+ ADC`（第一候補）または `Raspberry Pi Codec Zero`（国内入手性優先の代替）
- 2x20、2.54 mmピッチのGPIO積層ヘッダまたはロングピンGPIOエクステンダ
- M2.5、10〜16 mmのスペーサ4本
- HATの音声端子に合わせたRCAケーブル、3.5 mmケーブル、またはパネル取付ジャック
- HATと物理controlを同時に使用するための40ピンGPIOブレークアウトまたは延長ケーブル

HATはI2SでGPIO18〜21を使用するものが多い。MCP3008はSPI0のGPIO8〜11、BypassはGPIO17へ割り当て、HAT側のI2S信号と共有しない。HATごとに使用GPIO、device-tree overlay、対応kernel、full-duplex動作を確認してから採用する。HAT移行後も48 kHzを基準とし、128 frames/periodから開始して、xrun、round-trip latency、CPU load、入力飽和を実測する。

## 6. 物理コントロール設計（段階3向け）

Raspberry Pi 5はポテンショメータのanalog voltageを直接読めないため、外付けADCを使用する。初期候補は8 channelを持つSPI ADCのMCP3008とする。ADCや配線を変更しても `ControlSource` の実装だけで吸収する構造にする。

GPIOとADCのlogic/reference voltageは3.3 Vとし、Raspberry PiのGPIOへ3.3 Vを超える電圧を入力しない。potは3.3 VとGNDの間に接続し、wiperをADC inputへ入れる。Raspberry Pi、ADC、switchはGNDを共有する。switchはGPIOのpull-upを使ってGNDへ落とす構成を基本とする。

段階3の最小control set:

| control | 種別 | 対応parameter |
| --- | --- | --- |
| Depth | 10 kΩ linear pot | `depth` 0..1 |
| Time | 10 kΩ linear pot | `time` 0..1 |
| Upward | 10 kΩ linear pot | `upward` 0..1 |
| Downward | 10 kΩ linear pot | `downward` 0..1 |
| Bypass | momentaryまたはlatching switch | effect bypass |

Input Gain、Output Gain、crossoverは当初CLI値のままとし、必要性を感じてからpotを追加する。配線上はADCの空きchannelを確保しておく。

control処理の要件:

- GPIO/ADCをaudio callbackから直接読まない。
- control threadは250〜1,000 Hz程度で入力をsampleし、pot値にlow-pass smoothingとdead bandを適用する。
- switchはsoftware debounceする。
- control threadからaudio callbackへは、完全なparameter snapshotをlock-freeまたはbounded non-blocking queueで渡す（`docs/contracts.md` §6 と同じ非blocking境界を使う）。
- callbackは各process cycleの先頭で最新snapshotを最大1件だけ取り込み、per-sample smoothingへtargetとして設定する。
- control thread停止やADC read失敗時は最後の正常値を保持し、audio callbackを停止させない。

段階3のBypassはtrue bypassではなく、保存したDepthへ戻せるeffect bypassとする。

```text
bypass on  -> depth target = 0
bypass off -> depth target = saved physical Depth value
```

これによりraw inputとの位相差を持つcrossfadeを避け、LR4再合成経路を保ったままダイナミクスだけを無効化する（`docs/decisions/0004`）。電源断・OS停止・USB切断時にも音を通すtrue bypass relayはDIY完成条件に含めない。

### 6.1 初心者向けブレッドボード試作部品

段階3の初期試作は、はんだ付けを最小限にするためブレッドボードで行う。次を推奨購入品とする。販売店の在庫や型番は変動するため、購入時に国内在庫を再確認する。

| 部品 | 推奨メーカー・型番 | 数量 | 用途 |
| --- | --- | ---: | --- |
| ブレッドボード | サンハヤト `SAD-01`、948穴 | 1 | MCP3008、pot、switchの試作 |
| ADC | Microchip `MCP3008-I/P`、DIP-16 | 1 | 8ch / 10bit SPI ADC |
| DIPソケット | 16ピン ICソケット | 1 | MCP3008の保護・交換 |
| ポテンショメータ | 秋月電子 `SH16K4B103L20KCCI`、10 kΩ Bカーブ | 4 | Depth、Time、Upward、Downward |
| ツマミ | 6 mm軸用、例：`K-1-B` | 4 | ポテンショメータ操作 |
| Bypassスイッチ | 秋月電子 `MS-402-K`、モーメンタリ | 1 | パネル用の本命候補 |
| 試験用タクトスイッチ | `DTS-63-N-V-BLK`等 | 1 | パネルスイッチ前の動作確認 |
| ジャンパ線 | ブレッドボード用オス-メス15 cm | 20本程度 | GPIOとブレッドボードの接続 |
| ジャンパ線 | オス-オス | 40本程度 | ブレッドボード内配線 |
| デカップリングコンデンサ | 0.1 uF積層セラミック | 2〜5 | MCP3008電源安定化 |
| 測定器 | デジタルマルチメータ | 1 | 3.3 V、GND、短絡の確認 |

日本でいう直線型のポテンショメータはBカーブを選ぶ。Aカーブは使用しない。MCP3008のVDD、VREF、potの上端は3.3 Vへ接続し、potの下端、MCP3008のAGND/DGND、Raspberry Pi、switchはGNDを共有する。GPIO、ADC入力、VREFへ5 Vを接続してはならない。

`SAD-01`は948穴、117×83×9 mmのブレッドボードで、4ブロックの部品搭載領域とジャンプブロックを持つ。4個のpotはボード外へ配置してリード線で接続し、ブレッドボード上にはMCP3008、DIPソケット、電源レール、Bypass switch、GPIO配線を配置する。SAD-01でも配線が窮屈になった場合は2枚連結できる。小型ボードに無理に音声回路やHATを同居させない。

### 6.2 GPIOブレークアウトとピン割り当て

初心者はRaspberry Pi 5の40ピンヘッダへ配線を直接挿すより、40ピンGPIOブレークアウト基板、40ピンリボンケーブル、T型基板の組み合わせを使う。秋月電子の `AE-RBPI-BOB40KIT` は候補とするが、購入時はRaspberry Pi 5対応と在庫を確認する。

HATとの信号衝突を避ける初期割り当ては次のとおりとする。

| 信号 | Raspberry Pi GPIO | 用途 |
| --- | ---: | --- |
| SPI SCLK | GPIO11 | MCP3008 CLK |
| SPI MOSI | GPIO10 | MCP3008 DIN |
| SPI MISO | GPIO9 | MCP3008 DOUT |
| SPI CE0 | GPIO8 | MCP3008 CS |
| Bypass | GPIO17 | switch入力、内部pull-up使用 |
| ADC入力 | MCP3008 CH0〜CH3 | Depth、Time、Upward、Downward |

SPI0をGPIO8〜11へ置くことで、一般的なI2SオーディオHATが使用するGPIO18〜21との衝突を避ける。Bypass switchはGPIO17とGNDの間に接続し、外付けpull-up抵抗は初期構成では使用しない。

## 7. ハードウェア・物理コントロールの検証計画（未実施）

以下は、対応するハードウェアがまだ存在しないため未実施のテスト計画。段階2・3着手時に `docs/contracts.md` の該当セクションへ昇格するか、実施結果を別途記録する。

### 7.1 Raspberry Pi / Babyfaceテスト（段階2）

Raspberry Pi 5、Babyface CC mode、48 kHzで次を確認する。

1. ALSAとJACKからBabyfaceが認識され、capture/playback portのchannel mappingを記録できる。
2. 128 frames/periodの保守的な設定から開始し、30分間xrunなしで処理できる。
3. 64 frames/periodへ下げて同じ試験を行い、安定する最小値を記録する。64で不安定な場合は128を採用してよい。
4. release buildのJACK DSP loadとcallback時間に十分な余裕がある。通常演奏時のload 50%未満を目安とする。
5. analog loopbackでround-trip latencyを実測する。段階3の目標は10 ms以下とするが、数値だけでなく実際の演奏感も記録する。
6. `SafeStart` で開始し、出力にNaN/Inf、意図しないhard clipping、極端なlevel jumpがない。
7. phantom powerのon/offを含む実際の使用条件で、低電圧、USB reset、音切れが起きない。
8. CPU温度とthrottling状態を確認する。

### 7.2 物理コントロールテスト（段階3）

- fake `ControlSource` を使い、snapshotの欠落や順序変更があっても最新値へ収束すること。
- potの端点が正確に0と1へ到達し、中間値が単調に変化すること。
- ADC noise付近でparameterが細かく振動しないこと。
- knobを急に回してもclick/pop、NaN、xrunが発生しないこと。
- Bypass操作で音量の不連続がなく、解除時に現在のDepth位置へ戻ること。
- control threadを停止・再起動してもaudio処理が継続すること。
- 実機で1時間演奏し、操作性と不具合を短いメモに残すこと。

Raspberry Pi実機を扱う段階以降は、`cargo clippy --all-targets --all-features -- -D warnings` と `cargo test --all-targets --all-features` も成功させる（`pi-controls` feature込み）。

## 8. ロードマップ

開発は次の4段階で進める。段階1〜3を順番に行い、段階3をDIYプロジェクトとしての実用上の完成とする。段階4は必要性を感じた後に着手する任意作業であり、段階3の完了を妨げない。

### 8.1 段階1: PC上での仮説検証（v0.1）

目的:

- DSP構造と音の方向性が正しいかを、GPIOやRaspberry Pi固有問題から切り離して検証する。
- 参照実装のコピーではなく、本ノートの数式から独立した実装を完成させる。

作業:

1. `params`、数値変換、validationを実装する。
2. biquad、LR4 crossover、low branchのphase compensatorを実装し、再合成テストを通す。
3. envelopeとdual-threshold gain computerを実装する。
4. `BandProcessor` と `OttProcessor` を実装し、chunk不変性を確認する。
5. `SafeStart` と `Default` presetを実装する。
6. CLI、JACK adapter、終了処理を実装する。
7. 動作確認用に、port一覧・接続を行うexampleヘルパー（`list_ports`、`connect_ports`）を実装する。
8. PC上のJACKまたはPipeWire JACK互換環境で、QjackCtl等のGUIパッチベイまたはexampleヘルパーを使って接続し、楽器・音源・sine sweepを通して聴感を確認する。
9. dry/depth、Time、Upward、Downward、input/output gainが期待通り変化するか確認する。
10. CPU load、xrun、出力peakを記録する。

成果物:

- PCで実行できる `oxtt` CLI
- port一覧・接続用のexampleヘルパー（`list_ports`、`connect_ports`）
- 単体・統合テスト
- 短い試聴メモ。良かった素材、破綻した素材、調整したいparameterを記載する

完了条件:

- `cargo test --all-targets` と手動JACKスモークテスト（`docs/development.md`）が成功する。
- `SafeStart` で30分以上、xrunや異常出力なく処理できる。
- OTT風の効果が聴感で確認でき、Raspberry Piへ移す価値があると判断できる。

### 8.2 段階2: Raspberry PiでのCLI実行（v0.2）

目的:

- Raspberry Pi 5、JACK2、Babyface CC modeの組み合わせで、レイテンシーと安定性が演奏用途に足りるか確認する。
- 常駐化やGPIOを入れる前に、audio経路だけを確立する。

#### 8.2.1 方針

- Raspberry Pi OS Lite 64-bitのhost上で、既存のdistrobox `oxtt`（`docker.io/library/debian:bookworm`）をbuild専用に使う。JACKとrelease binaryの実行はPi hostで行い、hostの `/dev/snd` とrealtime権限を直接使う。コンテナもPi上の `aarch64-unknown-linux-gnu` native環境なので、`rust-toolchain.toml` に `targets = ["aarch64-unknown-linux-gnu"]` を追加せず、`cargo build --release --locked` にも `--target` を付けない。
- `rust-toolchain.toml` は開発・release buildで使うRustを固定する。`Cargo.toml` の `rust-version` はMSRVを表し、release binaryをMSRVでbuildする指定ではない。
- macOSからのcross buildは標準にしない。Rust targetの追加だけではLinux/AArch64 linker、sysroot、target用 `libjack`、`pkg-config` 設定が揃わないためである。Pi上のdistrobox build時間または反復deployが実測上の問題になった場合だけ、container imageと `Cross.toml` をまとめて導入する。hostで実行するため、hostとcontainerのlibc ABIが大きく異ならないことを確認する。
- 通常kernelとJACKのrealtime schedulingから開始する。RT kernelは、以下の権限、period、USB、電源、温度を確認してもxrunが残る場合の比較対象とし、最初から導入しない。
- 現時点では `justfile` を導入しない。共通のbuild/testは既存のCargo commandで足り、実機固有のALSA card名とJACK port名はまず観測が必要である。段階2の最後に実機値を固定した `scripts/run-pi.sh` を作り、macOSからのdeployなど3個以上の反復的な複合commandが生じた時点で `just` 導入を再検討する。

#### 8.2.2 OS imageと初回起動

1. macOSのRaspberry Pi ImagerでdeviceにRaspberry Pi 5、OSにRaspberry Pi OS Lite (64-bit)を選ぶ。hostname（以下では `oxtt-pi`）、user、timezone、network、SSH public keyをImagerで設定してからmicroSDへ書き込む。image名とImagerのversionを試験メモへ残す。
2. Active Coolerを取り付け、5 V / 5 A級電源を使用する。Babyfaceはまだ接続せずに起動する。
3. macOSから接続する。`<user>` はImagerで作成したuserへ置き換える。

   ```sh
   ssh <user>@oxtt-pi.local
   ```

4. OSを更新して再起動する。

   ```sh
   sudo apt update
   sudo apt full-upgrade
   sudo reboot
   ```

5. 再接続後、64-bit OSであることを確認する。`uname -m` は `aarch64`、`getconf LONG_BIT` は `64` でなければならない。

   ```sh
   uname -m
   getconf LONG_BIT
   cat /etc/os-release
   cat /etc/rpi-issue
   uname -a
   vcgencmd measure_temp
   vcgencmd get_throttled
   ```

   初期状態の `vcgencmd get_throttled` は `throttled=0x0` を期待する。0以外なら、audio試験より先に電源または冷却を直す。

6. host上でdistroboxの状態とrepositoryの配置を確認する。以降、`build`とRust commandは `distrobox enter oxtt` で入ったcontainer内、JACK、ALSA、release binaryの実行はPi hostで行う。`~/workspaces` はhostとcontainerで共有されるため、repositoryを別の場所へcloneしない。

   ```sh
   distrobox ls
   ls -la ~/workspaces/oxtt
   distrobox enter oxtt
   cd ~/workspaces/oxtt
   ```

   `oxtt` containerがまだ存在しない場合の作成例は次のとおりとする。containerはbuild専用なので、hostのsupplementary groupや `/dev/snd` を引き継ぐ `keep-groups` は指定しない。既存のcontainerが正常に動作している場合は作成し直さない。

   ```sh
   distrobox create --name oxtt --image docker.io/library/debian:bookworm
   ```

7. Pi hostでJACK実行とALSA/USB確認に必要なruntime packageを導入する。build用の `pkg-config`、`libjack-jackd2-dev`、Rust toolchainはcontainer側にだけ導入する。

   ```sh
   sudo apt update
   sudo apt install jackd2 alsa-utils usbutils file
   ```

   `jackd2`はhost側のJACK server、`libjack.so.0`、`jack_lsp`、`jack_connect`などのruntimeを導入する。`jackd2`の導入中に `Enable realtime process priority?` と聞かれた場合は `Yes` を選ぶ。ただし、最終的な値は8.2.3のhost側 `audio.conf` とlogin sessionで確認する。Debian bookwormでは `jack-example-tools` はhostにもcontainerにも必須ではない。

#### 8.2.3 hostのrealtime権限

realtime schedulingとPAM limitsは、distrobox containerではなくPi host側で設定する。container内で `apt install jackd2` を実行してもhostのlogin limitsは変更されない。

1. `distrobox enter oxtt` を終了し、Pi hostでaudio groupとlimitを設定する。

   ```sh
   exit
   id -nG
   getent group audio
   sudo usermod --append --groups audio <user>
   sudoedit /etc/security/limits.d/audio.conf
   ```

   `audio.conf` には次の2行を設定する。

   ```text
   @audio - rtprio 95
   @audio - memlock unlimited
   ```

2. login sessionへgroupとlimitを反映するためhostを再起動し、host側で確認する。

   ```sh
   sudo reboot
   ```

   ```sh
   id -nG
   ulimit -r
   ulimit -l
   ```

   `id -nG` に `audio` が含まれ、`ulimit -r` が0より大きく、`ulimit -l` が `unlimited` であることを確認する。

3. `oxtt`はbuild専用containerなので、hostのaudio groupとPAM limitsをcontainerへ引き継ぐ必要はない。container内でJACKまたは `/dev/snd` を実行・検証しない。

#### 8.2.4 containerのpackage、Rust、release build

1. 以降はcontainer内で実行する。`~/workspaces/oxtt` が作業directoryであり、host側の同じdirectoryと共有されていることを確認する。

   ```sh
   distrobox enter oxtt
   cd ~/workspaces/oxtt
   uname -m
   dpkg --print-architecture
   cat /etc/os-release
   ```

   `uname -m` は `aarch64`、`dpkg --print-architecture` は `arm64` でなければならない。`/etc/os-release` はcontainerのDebian bookwormを記録し、host OSのversionとは分けて扱う。

2. Rust buildとJACK bindingのcompileに必要なpackageをcontainerへ導入する。container内のpackage installなのでhostのapt packageは変更されない。ALSA device確認、JACK server、release binaryの実行に必要なpackageは8.2.2でhostへ導入する。

   ```sh
   sudo apt update
   sudo apt install build-essential pkg-config git curl file libjack-jackd2-dev
   ```

   `jack-example-tools` はOSのreleaseによって提供状況が異なる。Raspberry Pi OS Lite 64-bitの標準containerで使うDebian bookwormでは標準repositoryにないため、上のcommandには含めない。`jack_lsp`、`jack_connect` など、以降のJACK graph確認に必要な基本toolは`jackd2`から導入される。Debian trixieなど、対象OSのrepositoryで`apt-cache show jack-example-tools`が成功する場合だけ、必要に応じて次を追加実行する。

   ```sh
   apt-cache show jack-example-tools
   sudo apt install jack-example-tools
   ```

3. `rustup` をcontainer内へminimal profileで導入する。repository内の最初のRust commandが `rust-toolchain.toml` のversion、`clippy`、`rustfmt` を導入する。Rust toolchainはhostとcontainerで共有されないため、host側にRustがあっても省略しない。

   ```sh
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- --profile minimal --default-toolchain none
   source "${HOME}/.cargo/env"
   cd ~/workspaces/oxtt
   rustup show active-toolchain
   rustc -vV
   ```

   `rustc -vV` の `host` は `aarch64-unknown-linux-gnu` でなければならない。

4. repository rootでrelease binaryをbuildし、architectureとJACKのbuild/runtime環境を確認する。

   ```sh
   cargo build --release --locked
   file target/release/oxtt
   pkg-config --modversion jack
   pkg-config --libs jack
   ```

   `file` はAArch64のELFでなければならない。`pkg-config --modversion jack` はJACKのversionを表示し、`pkg-config --libs jack` は少なくとも `-ljack` を含まなければならない。これらはrelease binaryをbuildするdistrobox内で確認する。distrobox内に `ldconfig` がない場合もあるため、runtime libraryの確認に `ldconfig` を必須としない。

   ホストとdistroboxでは確認結果が異なる。hostはJACKを実行するruntime環境なので、`pkg-config`がなく `jack.pc` を見つけられない場合でも、`libjack.so.0` が存在すれば直ちに異常とはしない。hostでは次でruntime libraryを確認する。

   ```sh
   ldconfig -p | grep libjack
   ```

   distroboxはbuild環境なので、`libjack-jackd2-dev`と`pkg-config`を導入し、次の2 commandが成功することを要求する。distroboxの `ldconfig` の有無や、hostと異なるlibrary pathは問題にしない。

   ```sh
   pkg-config --modversion jack
   pkg-config --libs jack
   ```

   Rustの `jack` crate 0.13系はデフォルトでJACKを実行時に動的ロードする。そのため、`ldd target/release/oxtt` に `libjack.so.0` が表示されないことがある。`ldd`の出力だけでJACK依存を判定せず、containerでのbuild時 `pkg-config` 成功、hostの `libjack.so.0`、およびhostでのJACK server起動後のbinary実行で確認する。ここでbuildが失敗する場合はcross buildへ切り替えず、まずdistrobox内の `libjack-jackd2-dev` と `pkg-config` の導入を確認する。

#### 8.2.5 BabyfaceとALSA card名の確認

1. macOSまたは対応PCでBabyfaceのfirmwareを更新・確認し、manualに従ってCC modeへ切り替える。phantom powerをoff、monitor levelを最小にし、PiのUSB portへhubを介さず直接接続する。
2. Pi hostでUSB deviceと `/dev/snd` を確認する。host側packageは8.2.2で導入済みとする。

   ```sh
   lsusb
   ls -la /dev/snd
   ```

3. host側でALSA cardとcapture/playbackのdeviceを記録する。

   ```sh
   mkdir -p tmp/pi-stage2
   ls -la /dev/snd | tee tmp/pi-stage2/snd.txt
   cat /proc/asound/cards | tee tmp/pi-stage2/asound-cards.txt
   aplay -l | tee tmp/pi-stage2/aplay-l.txt
   arecord -l | tee tmp/pi-stage2/arecord-l.txt
   ```

   `/dev/snd`がpermission deniedになる場合は、host側の `audio` group、login session、ACL、Babyfaceの接続状態を確認する。containerのgroup引き継ぎ設定はこの実行経路では確認対象にしない。

4. `/proc/asound/cards` の角括弧内にあるcard名を記録する。以降の `<card-name>` はその値へ置き換え、boot順で変わり得る `hw:0` は使用しない。Babyfaceを抜き差しして同じcard名で再認識されることも確認する。
   現環境では `<card-name>` として `Pro73056544` を使用する。

   現環境でtest signalによるdirect loopbackから確定したport mappingは次のとおりである。JACKの `capture` はBabyfaceからhostへ入る信号、`playback` はhostからBabyfaceへ出る信号として扱う。`outN` / `inN`というALSA aliasだけから物理channelを推測せず、以下の対応は実機で聴取確認した結果として記録する。

   | 方向 | Babyface physical channel | JACK port |
   | --- | --- | --- |
   | input | XLR Mic/Line 1/2 | `system:capture_1/2` |
   | input | Line/Instrument 3/4 | `system:capture_3/4` |
   | input | ADAT 1/2 または SPDIF L/R | `system:capture_5/6` |
   | input | ADAT 3/4 | `system:capture_7/8` |
   | input | ADAT 5/6 | `system:capture_9/10` |
   | input | ADAT 7/8 | `system:capture_11/12` |
   | output | Main L/R | `system:playback_1/2` |
   | output | Phones L/R（Line 3/4） | `system:playback_3/4` |
   | output | ADAT 1/2 または SPDIF L/R | `system:playback_5/6` |
   | output | ADAT 3/4 | `system:playback_7/8` |
   | output | ADAT 5/6 | `system:playback_9/10` |
   | output | ADAT 7/8 | `system:playback_11/12` |

#### 8.2.6 JACK起動とport mapping

1. 以降のJACK、release binary、port操作はすべてPi hostで行う。最初のgraph確認は48 kHz、128 frames/period、3 periodsで行う。次のcommandをforegroundで実行したままにする。

   ```sh
   jackd -R -d alsa -d hw:CARD=<card-name> -r 48000 -p 128 -n 3 2>&1 | tee tmp/pi-stage2/jackd-128x3.log
   ```

   `cannot use real-time scheduling`、device open error、xrunが出る場合は先へ進まない。まず8.2.3のgroup/limit、card名、他processによるdevice使用を確認する。

2. 別のSSH sessionでJACKの状態と全portを記録する。

   ```sh
   cd ~/workspaces/oxtt
   jack_samplerate
   jack_bufsize
   jack_lsp -A | tee tmp/pi-stage2/jack-ports.txt
   ```

3. まずoxttを使わず、physical inputからheadphone outputへのdirect loopbackでmappingを検証する。現行の基準経路は次のとおりである。monitor levelを低くし、入力へtest signalを入れてから実行する。

   ```sh
   jack_connect system:capture_3 system:playback_3
   jack_connect system:capture_4 system:playback_4
   jack_lsp -c -A | tee tmp/pi-stage2/port-mapping-connections.txt
   ```

   In3/4の入力メーターとPhones L/Rの聴取結果が一致した場合、`system:capture_3/4`をLine/Instrument 3/4、`system:playback_3/4`をPhones L/Rとして `tmp/pi-stage2/port-mapping.md` へ記録する。direct loopback試験後は、oxttの接続試験前に必要に応じて切断する。

   ```sh
   jack_disconnect system:capture_3 system:playback_3
   jack_disconnect system:capture_4 system:playback_4
   ```

   他の物理channelを使う場合も、同じ手順で一組ずつ接続して聴取確認する。`system:capture_1`などの番号だけからanalog channelを推測しない。
4. containerでbuildしたrelease binaryを、共有workspaceからhost上で `SafeStart` として起動する。

   ```sh
   ./target/release/oxtt --preset safe-start
   ```

5. さらに別のSSH sessionで、記録したport名を使って接続する。

   ```sh
   jack_connect system:capture_3 oxtt:input_l
   jack_connect system:capture_4 oxtt:input_r
   jack_connect oxtt:output_l system:playback_3
   jack_connect oxtt:output_r system:playback_4
   jack_lsp -c
   ```

   monitor levelを低くしたまま入力し、左右、入出力level、無音時の異常出力、click/popがないことを確認する。`Default` presetはこの確認が終わるまで使わない。

#### 8.2.7 period設定と30分安定性試験

##### 暫定実機結果（通常USB-PD電源）

5 V / 5 A級電源が未導入のため、通常のUSB-PD電源で行った暫定試験結果を次のように記録する。これは正式な30分安定性試験の合格記録ではなく、設定候補を絞るための短時間観測である。

- `vcgencmd get_throttled`: `throttled=0x50000`
- `vcgencmd measure_clock arm`: `frequency(0)=2400023808`（測定時はほぼ定格性能）
- 温度: 起動直後51℃、動作中56〜59℃程度で安定
- 平常時の `jack_cpu_load`: 0.1〜0.3程度

`throttled=0x50000` は、現在のthrottlingだけを示す値ではなく、起動後にundervoltageとthrottlingが発生した履歴を含む値として扱う。したがって、通常USB-PD電源でのxrun結果は電源条件に依存する可能性があり、5 V / 5 A級電源で再試験するまで製品相当の安定性判断には使わない。

| JACK設定 | `jack_cpu_load` | oxtt xrun | 観測されたJACK log |
| --- | ---: | ---: | --- |
| 128 x 2 | 3〜4.5 | 4回 | `client = oxtt was not finished, state = Triggered` 4回 |
| 128 x 3 | 3〜6.5 | 3回 | `state = Running` 1回、`state = Triggered` 2回 |
| 256 x 3 | 2.8〜4.3 | 0回 | なし |

##### 暫定結果のxrun分析

この試験で実行したbinaryはcommit `45e7dd6` でbuildしたrelease binaryである。したがって、commit `a2604cb` で修正済みの「静止したcrossoverにも毎sample filter係数を再計算し、`sin_cos` を呼ぶ」問題は今回のxrunの原因ではない。現行の `Crossover::process_frame` はcutoff targetまたはsample rateが変わった遷移中だけ係数を更新し、静止後は更新を止める。

48 kHzにおける1 callbackの公称時間は、64 framesで約1.33 ms、128 framesで約2.67 ms、256 framesで約5.33 msである。`128x2`/`128x3`でJACKが `client = oxtt was not finished` を記録し、`256x3`で記録しなかったことは、平均CPU不足よりも、`oxtt` callbackがまれに128-frameの締切を外すことを示す。`jack_cpu_load` 3〜6.5は平均負荷の指標であり、最悪callback時間やschedulerによる一時的な遅延を否定しない。したがって64x3（約1.33 ms）を候補として扱う根拠は現時点ではなく、128x3の正式試験を通過するまで試験対象にしない。

今回の電源条件には強い交絡がある。`throttled=0x50000`は起動後にundervoltageとthrottlingが起きた履歴であり、測定時にarm clockが約2.4 GHz、温度が56〜59℃で安定していても、当該boot全体を電源上健全とは扱えない。BabyfaceをUSB接続しているため、Piの5 V / 5 A級電源、Babyfaceの外部電源、USB reset/undervoltageのないkernel logを確認した再試験まで、PiのCPU性能やJACK設定だけを原因と結論しない。通常のUSB-PD電源は、5 V / 5 A PDOをPiへ実際に供給できず、USB peripheralの電力余裕が不足する可能性がある。

一方で、crossover以外にも64-frame動作の余裕を減らし得るDSP上の改善候補が残る。現在の処理は、parameterが静止していてもsampleごとにinput/output gainと各bandのmakeup gainのdB変換、threshold power、envelope係数、envelope levelのdB変換を計算する。これらのうち、threshold power、makeup gain、attack/release係数、floor powerは、対応するsmoothed parameterが変わった時だけ再計算してcacheできる。これは今回のxrunの主因と確定したものではないため、先に電源・RT条件を正した同一条件のprofileで、callbackの最悪時間とlibm呼出しの寄与を確認してから最適化する。

原因を次のように切り分ける。正式試験では、(1) `oxtt`を外したphysical direct loopback、(2) 同じJACK設定の`oxtt`経由、を別々に30分試験する。(1)でもxrunするなら電源、USB、kernel、JACK realtime schedulingを優先して調べる。(2)でのみxrunするならDSPまたはoxtt client schedulingをprofileする。毎試験でcommit hash、JACK設定、period、電源、`get_throttled`、kernel USB log、JACK log、`oxtt` xrun countを一組の記録として残す。

暫定判定として、128 x 2と128 x 3はxrunが発生したため不採用とする。256 x 3は今回の電源・温度条件でxrun 0回だったため次の正式試験候補とするが、採用確定には5 V / 5 A級電源での30分試験、`oxtt`側とJACK log側の両方のxrun集計、undervoltage履歴のない状態での再確認を要求する。

1. 正式試験の前に、`oxtt` の `NotificationHandler::xrun` でxrun通知を数えるdiagnostic counterを実装する。callback内では共有 `AtomicU64` をincrementするだけにしてI/Oを行わず、表示はmain threadだけが行う。通常運用では出力しない。段階2の正式試験では `--report-xruns-on-exit` を付け、正常終了時に次の形式でstderrへ1行出力する。xrun通知が一度もなくても `0` を出力し、summary自体がない試験は判定不能として不合格にする。

   ```text
   oxtt: xrun_count=0
   ```

2. 正式試験は、5 V / 5 A級電源と安定した冷却を確認したうえで、暫定結果の候補である `256x3` から行う。比較対象として `128x2`、`128x3` を再試験する場合も、電源条件と試験時間を混同せず別記録にする。

   ```sh
   jackd -R -d alsa -d hw:CARD=<card-name> -r 48000 -p 256 -n 3 2>&1 | tee tmp/pi-stage2/jackd-256x3.log
   ```

3. 別sessionで `oxtt` を30分後にSIGTERMで正常終了させ、8.2.6の4接続を行う。stderrも保存するため `2>&1` を外さない。

   ```sh
   timeout --signal=TERM 30m ./target/release/oxtt --preset safe-start --report-xruns-on-exit 2>&1 | tee tmp/pi-stage2/oxtt-256x3.log
   ```

4. Pi hostの別sessionでJACK DSP loadを観測する。同時に、別sessionでCPU温度とthrottlingを観測する。開始時、通常演奏時、終了直前の値を試験メモへ残す。`jack_cpu_load` は負荷の観測用であり、xrun件数の判定には使用しない。

   ```sh
   jack_cpu_load
   ```

   次のcommandはPi host側で実行する。container内では `vcgencmd` が利用できない場合がある。

   ```sh
   watch -n 5 'vcgencmd measure_temp; vcgencmd get_throttled'
   ```

5. 終了後、`oxtt` が出力したxrun件数を確認する。1つ目のcommandは `1`、2つ目は `oxtt: xrun_count=0` を1行だけ表示しなければならない。

   ```sh
   grep -Fc 'oxtt: xrun_count=' tmp/pi-stage2/oxtt-256x3.log
   grep -Fx 'oxtt: xrun_count=0' tmp/pi-stage2/oxtt-256x3.log
   ```

6. `jackd` logも別経路の証跡として機械的に集計し、集計結果を保存する。`jack_log_xrun_matches=0` でなければ不合格とし、0以外の場合は2つ目のcommandで該当行と行番号を確認する。`grep` に何も表示されなかったという目視だけで合格にしない。

   ```sh
   awk 'tolower($0) ~ /(xrun|underrun|overrun)/ { count++ } END { print "jack_log_xrun_matches=" count }' tmp/pi-stage2/jackd-256x3.log | tee tmp/pi-stage2/jackd-256x3-xrun-count.txt
   grep -Ein 'xrun|underrun|overrun' tmp/pi-stage2/jackd-256x3.log
   ```

7. Pi hostの別sessionでkernel logとthrottling状態を確認する。次のcommandはcontainer内ではなくhost shellで実行する。

   ```sh
   # Pi host
   sudo journalctl -k --since '40 minutes ago' | grep -Ei 'usb|reset|undervoltage|voltage|throttl'
   vcgencmd get_throttled
   ```

8. 合格には `oxtt: xrun_count=0` と `jack_log_xrun_matches=0` の両方を要求する。値が一致しない場合も見逃さず、両方のlogを残して原因を調べる。さらにUSB reset、undervoltage、thermal throttlingがないことを確認する。
9. 正式試験で `256x3` が安定した場合、同じ3 periodsを保ったまま `-p 128`、必要に応じて `-p 64` へ変更して比較する。64で不安定なら256へ戻す。period数、電源条件、試験時間を混同せず、`256x3`、`128x3`、`64x3` のように組み合わせごとに記録する。

#### 8.2.8 round-trip latency試験

1. speakerまたはheadphoneを外し、phantom powerをoff、output levelを低くする。Babyfaceの測定対象analog outputをanalog inputへ適切なcableでloopbackする。
2. 採用候補のperiod設定でJACKを起動し、`jack_iodelay` を実行する。

   ```sh
   jack_iodelay
   ```

3. 別sessionで測定portを接続する。`<playback-port>` と `<capture-port>` は8.2.6で確認した同一loopback channelのportへ置き換える。

   ```sh
   jack_connect jack_delay:out <playback-port>
   jack_connect <capture-port> jack_delay:in
   ```

4. 表示される `total roundtrip latency` のframesとmsを記録する。安定性試験の採用候補である256と、比較候補の128 framesで測定し、演奏時の体感も併記する。JACK2のdefault asynchronous modeではJACK bufferに暗黙の1 periodが加わるため、`frames/period * periods` だけを実測round-trip latencyとして扱わない。
5. `oxtt` を含む経路も測る場合は、`--depth 0`、0 dB入出力で起動し、`jack_delay:out -> oxtt:input_l -> oxtt:output_l -> Babyface output -> cable -> Babyface input -> jack_delay:in` と接続する。

   ```sh
   ./target/release/oxtt --preset safe-start --depth 0 --input-gain 0 --output-gain 0
   jack_connect jack_delay:out oxtt:input_l
   jack_connect oxtt:output_l <playback-port>
   jack_connect <capture-port> jack_delay:in
   ```

#### 8.2.9 launch scriptと試験記録

1. ALSA card名、JACK port mapping、採用period数が確定した後に `scripts/run-pi.sh` を追加する。scriptはPi host上でJACKを採用設定で起動し、server readinessを待ち、`oxtt --preset safe-start` を起動して4 portを接続し、SIGINT/SIGTERM時に子processを終了する。観測前にplaceholderを推測してscript化しない。
2. `scripts/run-pi.sh` はPi hostで次の1 commandにより起動できることを確認する。この段階ではsystemd unitを作らない。

   ```sh
   cd ~/workspaces/oxtt
   ./scripts/run-pi.sh
   ```

3. `tmp/pi-stage2/` に最低限次を残す。
   - OS image名、`/etc/os-release`、kernel、Pi firmware、Rust、JACK、Babyface firmware version
   - ALSA card名とBabyface capture/playback port mapping
   - 試した `frames x periods` ごとの `oxtt` xrun count、`jackd` log上のxrun該当数、JACK DSP load、最高温度、`get_throttled`、kernel USB error
   - hardware-onlyと `oxtt` 経由のround-trip latency、演奏時の体感
   - 採用設定と、不採用設定を戻した理由

   systemとpackageのversionは、container内の情報とPi hostの情報を分けて保存する。Babyface firmware versionはfirmware更新に使用したmacOSまたはPC側で確認して追記する。

   ```sh
   # container内
   cd ~/workspaces/oxtt
   date -Is | tee tmp/pi-stage2/date.txt
   uname -a | tee tmp/pi-stage2/uname.txt
   cat /etc/os-release | tee tmp/pi-stage2/os-release.txt
   rustc -vV | tee tmp/pi-stage2/rustc.txt
   jackd --version | tee tmp/pi-stage2/jackd-version.txt
   dpkg-query -W build-essential pkg-config alsa-utils jackd2 libjack-jackd2-dev jack-example-tools | tee tmp/pi-stage2/packages.txt
   ```

   ```sh
   # Pi host
   vcgencmd version | tee ~/workspaces/oxtt/tmp/pi-stage2/pi-firmware.txt
   uname -a | tee ~/workspaces/oxtt/tmp/pi-stage2/host-uname.txt
   cat /etc/os-release | tee ~/workspaces/oxtt/tmp/pi-stage2/host-os-release.txt
   ```

成果物:

- Raspberry Pi用release binary
- Babyfaceのport mapping
- 1 commandで起動できる手動launch script
- 採用したsample rate、period size/count、round-trip latency、xrun結果のメモ

完了条件:

- 48 kHz / 128 frames以下の30分試験で、`oxtt: xrun_count=0` と `jack_log_xrun_matches=0` の両方を記録できる。summaryが欠落した試験は合格にしない。
- release buildの通常演奏時JACK DSP loadが概ね50%未満。
- latencyが演奏上許容できる。10 ms以下を目標とし、実測値を残す。
- USB reset、低電圧、thermal throttlingが通常使用時に発生しない。

### 8.3 段階3: ハードウェアスイッチ／ポテンショメータ連携（v0.3）

目的:

- PCやSSHからparameterを操作せず、机上で演奏に使えるDIYエフェクターにする。
- 常駐化よりも操作感と演奏中の安定性を優先する。

作業:

1. `ControlSource` とparameter snapshotの受け渡しをfake inputで実装・テストする。
2. `pi-controls` featureと `rppal` adapterを追加する。
3. MCP3008等のADCをSPI接続し、4つのpotをDepth、Time、Upward、Downwardへ割り当てる。
4. GPIO switchをBypassへ割り当て、debounceとDepthの保存・復帰を実装する。
5. ADCの実測最小・最大値を使ってcalibrationし、dead bandとsmoothingを調整する。
6. audio callbackとcontrol threadの間にlockやblocking処理がないことを確認する。
7. breadboardまたは簡易panelへ固定し、ケーブル抜けや短絡が起きにくい状態にする。
8. 手動launch scriptで起動し、7.2節のテストと1時間の演奏試験を行う。

成果物:

- 物理control付きRaspberry Pi実機
- GPIO/SPI pin assignment図
- ADC calibration値とcontrol mapping
- 机上演奏時の設定・不具合メモ

完了条件:

- PCやSSHによるparameter操作なしで演奏できる。
- knobとBypassがclick/popやxrunなしで操作できる。
- 1時間の演奏で音切れ、USB reset、control停止がない。
- launch自体はSSHまたは端末からの1 commandでよく、自動起動は要求しない。

### 8.4 段階4: 組み込み・常駐化（任意、低優先度）

目的:

- 電源投入後に手動loginせず使いたくなった場合だけ、段階3の実機をappliance化する。

候補作業:

1. JACKと `oxtt` をsystemd unit化し、起動順序とrestart policyを定義する。
2. Babyfaceを安定したALSA card名で開き、portを自動接続する。
3. Babyface未接続時は再試行し、接続後にaudio graphを復旧する。
4. 起動・終了時にoutputを短くramp/muteし、popを防ぐ。
5. Raspberry Piのpower buttonまたは専用GPIOでgraceful shutdownできるようにする。
6. xrun、USB disconnect、低電圧、温度を非RT threadで記録する。
7. 必要ならケース、panel、strain relief、外部電源配線を整える。

完了条件は利用者が必要になった時点で決める。read-only root filesystem、watchdog、relay true bypassなどの製品グレード機能は、このDIYプロジェクトでは必須としない。

## 9. 将来拡張

段階3の完成後、実際に演奏して必要性が明らかになったものだけ追加する。

1. Input Gain、Output Gain、crossover用potまたはrotary encoder
2. clipping、xrun、bypass状態を示すLED
3. 帯域threshold、amount、make-up gainのCLI/設定ファイル公開
4. optional safety limiterまたはsoft clip
5. selectable stereo link、sidechain
6. 4-band mode
7. CLAP/VST3 frontend
8. GUIと入出力／帯域別gain meter
9. 任意のpost stage（低域saturation、高域exciter）

post stageはコアコンプレッサーと別モジュールにし、有効化しない限り音へ影響させない。

## 10. 購入順序

購入は段階に合わせて分ける。HATとケースは、音声経路の検証結果が出るまで購入を急がない。

1. `MCP3008-I/P`、10 kΩ Bカーブpot 4個、試験用switch、ブレッドボード、ジャンパ線、0.1 uFコンデンサ、テスター
2. Raspberry Pi 5、安定した5 V / 5 A級USB-C電源、Active Cooler、microSDカード、GPIOブレークアウト
3. 段階2でBabyfaceを使ったJACK経路、xrun、latency、温度、電源を検証
4. HATへ移行する場合だけ、`HiFiBerry DAC+ ADC`または`Raspberry Pi Codec Zero`、GPIO積層ヘッダ、スペーサ、音声ケーブルを購入
5. 段階3の1時間演奏試験が完了した後に、ケース、パネル、ノブ、strain reliefを追加

電気的な試作段階では、HAT用の音声信号とpot用の制御信号を同じブレッドボード上で混在させない。HATのanalog input/outputはラインレベル音声用であり、potのwiperやGPIOへ接続してはならない。ギターなどのinstrument level信号を直接入力する場合は、別途DIまたはinstrument preampが必要になるため、現段階の購入品には含めない。

## 11. 参考資料

- [RME Babyface Pro FS User's Guide](https://rme-audio.de/downloads/bface_pro_fs_e.pdf) — CC mode、電源、入出力、sample rate
- [RME Babyface Pro FS support](https://rme-audio.de/babyface-pro-fs.html) — firmwareと公式manual
- [Raspberry Pi OS documentation](https://www.raspberrypi.com/documentation/computers/os.html) — Lite/64-bit構成
- [Raspberry Pi hardware documentation](https://www.raspberrypi.com/documentation/computers/raspberry-pi.html) — Raspberry Pi 5の電源と冷却
- [rustup toolchain overrides](https://rust-lang.github.io/rustup/overrides.html) — `rust-toolchain.toml` のchannel、component、profile、target
- [Debian jackd2 package](https://packages.debian.org/trixie/arm64/jackd2) — arm64版JACK2と関連package
- [JACK realtime scheduling](https://jackaudio.org/faq/linux_rt_config.html) — RT権限設定
- [JACK latency](https://jackaudio.org/faq/no_extra_latency.html) — JACK層のlatency特性
- [jack_iodelay manual](https://manpages.debian.org/unstable/jackd2/jack_iodelay.1.en.html) — analog loopbackによるround-trip latency実測
- [JACK device naming](https://jackaudio.org/faq/device_naming.html) — 安定したALSA card名
- [`rppal` documentation](https://docs.rs/rppal/latest/rppal/) — Raspberry Pi 5のGPIO/SPI/I2C access
- [秋月電子 MCP3008-I/P](https://akizukidenshi.com/catalog/g/g109485/) — 8ch / 10bit SPI ADC
- [サンハヤト SAD-01](https://shop.sunhayato.co.jp/products/sad-01) — 948穴ブレッドボード
- [秋月電子 可変抵抗器一覧](https://akizukidenshi.com/catalog/c/cvolume/) — 10 kΩ Bカーブ `SH16K4B103L20KCCI`
- [HiFiBerry DAC+ ADC](https://www.hifiberry.com/shop/boards/dacplus-adc/) — stereo ADC/DAC HAT
- [Raspberry Pi Codec Zero](https://www.raspberrypi.com/products/codec-zero/) — 公式I2S audio HAT
