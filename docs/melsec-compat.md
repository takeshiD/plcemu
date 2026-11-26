# MELSEC iQ-R 対応範囲と相違点

出典は三菱電機の公式マニュアルです。ページ番号は本文の印刷ページを示します。

- P-CPU — [CPU Module Instructions, Standard Functions/Function Blocks、SH-081266ENG-AC](https://www.mitsubishielectric.com/dl/fa/document/manual/plc/sh081266eng/sh081266engac.pdf)。§1.2 データ表記、§5.1–5.3 接点・結合・出力、pp. 168–176 タイマ・カウンタ、6章 比較・算術・転送。
- U-APP — [CPU Module User’s Manual (Application)、SH-081264ENG-AR](https://dl.mitsubishielectric.com/dl/fa/document/manual/plc/sh081264eng/sh081264engar.pdf)。§1.3（p. 54）で入出力のデバイスメモリと実機モジュールへのリフレッシュを区別。§22.1（p. 374）にデバイス一覧。
- P-DES — [Programming Manual (Program Design)、SH-081265ENG-R](https://www.mitsubishielectric.com/dl/fa/document/manual/plc/sh081265eng/sh081265engr.pdf)。§5.1（pp. 45–46）に直列・並列、a/b接点、パルス接点、コイルおよび実行順序の例。

表の挙動は plcemu のモデルを示します。境界条件の実行テストは `src/tests.rs` にあります。

| 項目 | plcemu の挙動 | 出典 | 相違・制約 |
|---|---|---|---|
| a/b接点 LD/LDI/AND/ANI/OR/ORI | 実装 | P-CPU §5.1、P-DES §5.1 | — |
| ANB/ORB, MPS/MRD/MPP | 実装 | P-CPU §5.2 | — |
| OUT/SET/RST | 実装（RSTはY/M/T/C/D） | P-CPU §5.3 pp. 166, 179, 181 | — |
| 立上り/立下り LDP/LDF/ANDP/ANDF/ORP/ORF | 命令ごとの前回値と比較し1スキャンだけON | P-CPU §5.1 p. 153 | — |
| デバイス表記 | X/Y=16進(X1F), M/T/C/D=10進, K=10進定数, H=16進定数, TS/TN/CS/CN | U-APP §22.1、P-CPU §1.2 | L,B,W,SM,SD,Z,R等は未対応 |
| スキャン順序・可視性 | スキャン先頭で入力リフレッシュ(Xは次スキャンで反映)、ラング上→下で実行、OUT結果は同スキャンの後続ラングから即参照可 | U-APP §1.3、P-DES §5.1 | 出力リフレッシュは区別なし(結果は常にEND後に観測) |
| タイマ OUT T K | 低速タイマ単位 `timer_unit_ms`(既定100ms)。コイルON後の経過時間=スキャン時間×(実行回数-1)で計時、設定値到達で接点ON。OFFで現在値0 | P-CPU §5.3 p. 168 | 実機のタイマ値は設定した単位とスキャン処理に依存する。固定時間で近似し、積算タイマSTと高速タイマ命令は対象外。 |
| カウンタ OUT C K | 入力の立上りで+1、設定値で停止・接点ON、RSTで0 | P-CPU §5.3 p. 174 | — |
| MOV/MOVP | 実装 | P-CPU 転送 | 32bit(DMOV)未対応 |
| + - * / (ADD等別名, P付き) | 16bit符号付き。+,-はラップアラウンド(32767+1=-32768)。* は32bit結果を D,D+1。/ は商D・余りD+1(0方向切捨て) | P-CPU 算術演算 | 0除算は演算エラーとして停止(実機のエラーコード・継続/停止設定は未模擬) |
| 比較 LD=/<>/>/</>=/<= (AND/OR) | 16bit符号付き比較 | P-CPU 比較演算 | 32bit(LDD=)未対応 |
| 範囲外定数 K40000 | 読込エラー | — | — |

FB・実機接続・GX Worksネイティブ形式は対象外。
