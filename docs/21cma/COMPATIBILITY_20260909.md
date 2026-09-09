# 21CMA 上游兼容性与生产程序复查 — 2026-09-09

本次在之前非校准开发的基础上，合入上游 main 并重新检查原生 21CMA 校准、解应用、模拟、减源、
CPU/CUDA 电离层 peeling、MS 转换/平均、源表工具和解格式转换/绘图。结果支持在已验证的
MS/XX 路径使用；完整逐源逐天线 Jones peeling 与全天科学成像结论仍不在本次保证范围内。

数值、二进制指纹、测试命令和完整本机日志路径见 [机器可读记录](compatibility_20260909.json)。
此前开发的背景和使用示例见 [功能指南](NON_CALIBRATION.md) 与 [首次验证记录](VALIDATION_20260909.md)。

## 上游检查

- 已合入 [上游 main 的 d46587d](https://github.com/MWATelescope/mwa_hyperdrive/commit/d46587dfb4a0453de5e632a7b6f7053c79d7b183)，提交日期为 2026-09-06（UTC）。
  相对之前的 v0.8 基线，主要是 Clippy 修复及 CI 触发条件；合并时保留了 21CMA 波束说明。
- 检查时最新正式版本仍为 [v0.8.0](https://github.com/MWATelescope/mwa_hyperdrive/releases/tag/v0.8.0)。
- `20260901_deps` 分支的 `cdbbf55a6935143494bb1023147161b7fb0e18c9` 尚未进入 main，
  涉及 Rust 1.88、Marlu 0.19、Hifitime 4 等依赖迁移。本项目的 vendored Marlu 含 21CMA
  精确时间/频率与 MS 写入改动，完整升级需要单独移植验证，因此本轮保留经过验证的依赖组合。
- 单独移植了其中 [422e92e 的选源排序修复](https://github.com/MWATelescope/mwa_hyperdrive/commit/422e92e)：
  `srclist-by-beam --collapse-into-single-source` 删除主源时维持剩余亮度顺序，避免有限源数时选错源。
  新增回归检查确认选中次亮源。
- `peel_c_ffi` 未合并分支未引入；它不提供本任务所缺少的完整 Jones peeling 实现。

## 复查发现并修复的问题

1. **`.MS` 后缀兼容**：模拟模板和 MS 输出原先拒绝生产数据常用的大写后缀，现在大小写均可使用。
2. **解时间块选择**：缺少逐块时间的多时间块 AO 解原先可能始终使用第一块；现在按观测跨度匹配。
   完整逐块起止时间可独立于平均时间使用；间隙和范围外时间选最近质心/区间，最后时刻选最后一块。
   添加不完整时间、范围外时间、单时刻、NaN 比例以及超过 100 块的端点回归。
3. **校准解质心**：非校准路径已保留实际时间，但校准 FITS 平均时间仍可能取整偏移 5 ms。
   现在使用同一望远镜感知的时间平均函数；原生不规则时间、读入平均与校准分块联合检查通过。
4. **全标记时间段**：MS 默认读取范围原先遇到内部全标记时段会丢弃后续有效数据；现在只裁去两端
   全标记时段，保留内部时段的逐样本标记。新回归同时包含首尾和内部全标记时间。
5. **原生大 MS 初始扫描**：从两端寻找有效范围，避免逐时刻扫描整个观测。本机约 1995 万行的
   原生 MS，在相同选择上初始读取由约 318 秒降至整条转换命令 5.4 秒。机器有并行任务，该数值
   仅是本机排查记录。直接读取完整 MS 的输出与独立 casacore 选行后转换的输出逐数组一致。

6. **CPU/CUDA 拟合拒绝策略**：扩大样本后发现，GPU 拒绝超限拟合，但 CPU 接受同样的拟合，
   导致后半段结果不同。现在两条路径共用原 GPU 限制：`|alpha|, |beta| <= 1e-3`、
   `0 <= gain <= 1.5`，拒绝时保留上一次通过的参数，并警告具体源、时间块和原因。
   过大的增益注入在 CPU/CUDA 测试中均应被拒绝，输出保留未校正残差。新增跨设备比较脚本，
   修复前它确实检测到真实 peeling 的 CPU/CUDA 差异达到输入 RMS 的 1.50%。
7. **GPU 临时内存**：释放每次电离层拟合分配的临时参数，修复随源数、时间块和 pass 数累积的显存泄漏。

8. **GPU 平均次序**：原先 GPU 在已经平均的模型上迭代相位，而 CPU 每次先在原始时间/频率
   采样上应用相位再平均。两者对宽时间块并不等价。GPU 现在每轮也在原始网格更新模型，
   与 CPU 一致。新增相隔 1800 秒的采样注入测试，检查位移、增益和残差恢复。

主要兼容性修复为 `a6f16c3`，拟合策略统一为 `02b29d0`，最终源代码提交为
`299a46c49b3bd07210cded1105723f0b889c6070`。

## 验证内容与结果

- CPU 库测试 **321/321**，CUDA 库测试 **379/379**，CLI 集成测试 **24/24**。
  干净源码 checkout 的严格 Clippy、Rust 格式和 diff 检查通过。
- **原生校准数据**：2013-03-25 的 `ms_5632:6144.MS`，选第 `0,1,2,3,10,11,12` 个时刻，
  40 天线、512 频道、5460 条互相关行。人工注入随天线、频率和时间块变化的复增益，再以读入
  时间/频率平均因子 2、校准时间块 4 求解。FITS 维度为 `2 × 40 × 256 × 8`，逐块起止时间
  和平均时间符合独立 Astropy 计算；应用到原生网格后相对 RMS 误差 **1.486×10⁻⁶**，CPU/CUDA 一致。
- **真实校准回归**：使用生产 NEC2++ 波束表和现有天空模型的前 100 个源、150 λ 最短基线。
  XX 有限解比例 **98.828125%**，未求解频道保留无效解处理。CUDA 新旧生产版的有限值掩码完全一致，
  有限解数值相对 RMS 差异 **0**。新程序应用解后 MS 可被 WSClean 读回并产生有限值脏图。
- **AO 解应用**：三块缺失时间信息的解，分别直接使用 `.bin` 和转为 FITS 后使用，均符合独立计算的
  时间跨度匹配和 Jones 乘积，误差为 0。该回退不能恢复 AO 格式未保存的不规则分块信息。
- **扩大非校准样本**：先前 B07 已校准数据的 16 个分散时刻，覆盖整个 100 时刻输入，12480 行、
  120 频道；时间平均 3 产生 6 个块，最后块为 1 个时刻，频率平均 7 的最后块为 1 个频道。
  CPU/CUDA 均检查真实减源与 peeling、1.1 倍亮源增益注入、保留非拟合源，以及输出时间、
  分数 Hz 频率、XX 元数据、标记与权重。量化结果保存在 JSON；CUDA 另外检查解往返与绘图。
  最终 CPU/CUDA 真实 peeling 输出相对差异 RMS 为 **1.146×10⁻⁷**，跨设备比较脚本全部通过。

拟合拒绝警告必须检查；JSON 中的初始/沿用参数不能解释为该时间块成功求解。
该样本 peeling 的整体残差不一定比直接减源更低；两者优化目标与限制不同。
真实残差 RMS 降低只说明这套处理链的表现，不能单凭它推断科学成像改善。WSClean 使用
`-niter 0` 检查文件兼容性，不作为最终图像质量验收。

## 本机运行与复现

项目生产脚本实际调用的程序位置为：

```text
/home/zhaofyastro/21cma_hyperdrive_project/mwa_hyperdrive-21cma/target-cuda-release/production/hyperdrive
```

该入口安装经过最终 CPU/CUDA 流程复核的 `299a46c` CUDA release 构建，使用 `--cpu` 可强制 CPU。
二进制 SHA-256：`f14511136b7e631987ca6184751fc8def5feb0079fadc8afc72c58ae02266553`。
文档提交可晚于此源码提交；程序启动日志显示 `299a46c` 且没有 dirty 标记。

裸命令 `hyperdrive` 当前解析到系统 `/usr/bin/hyperdrive`，是旧的 0.3.0；运行时应明确使用上述
项目路径。旧项目生产程序保存为下面验证根目录中的 `bin/hyperdrive-production-before-20260909`。
如需回滚，将此备份复制到项目生产入口的临时文件，再原子重命名替换入口即可。

全部验证产物位于：

```text
/data/zfy/21cma_hyperdrive_project/compatibility_validation_20260909/
  bin/hyperdrive-299a46c
  release_final/cal_cpu/
  release_final/cal_cuda/
  release_final/noncal_cpu/
  release_final/noncal_cuda/
  logs/
  production_install.json
```

每个最终流程目录保存 `commands.json`、`report.json`、逐命令日志及 MS/FITS；JSON 汇总中保存了
顶层复现命令。输入只读，所有注入/应用操作使用独立输出目录。原始观测、大型波束与生成文件不提交 Git。

校准检查使用 [`scripts/validate_21cma_calibration.py`](../../scripts/validate_21cma_calibration.py)，
需要 NumPy、python-casacore、Astropy；非校准检查使用
[`scripts/validate_21cma_noncal.py`](../../scripts/validate_21cma_noncal.py)。运行模板：

```bash
CUDA_VISIBLE_DEVICES=1 RAYON_NUM_THREADS=4 \
python scripts/validate_21cma_calibration.py \
  --hyperdrive /path/to/verified/hyperdrive \
  --input-ms /path/to/native.MS --source-list /path/to/sky.yaml \
  --beam-file /path/to/production-beam.h5 \
  --output-dir /path/to/new-calibration-check \
  --baseline-hyperdrive /path/to/old/hyperdrive \
  --wsclean /usr/local/bin/wsclean
```

省略两个可选程序参数可跳过旧版对照与图像读回；加 `--cpu` 检查 CPU 路径。每次输出目录须全新。
构建使用 Rust 1.87、`--locked --release --features cuda`、A100 compute 80，GPU 1 和 4 个 Rayon 线程。

使用 [`scripts/compare_21cma_noncal.py`](../../scripts/compare_21cma_noncal.py) 对两份完整输出目录
逐文件比较 CPU/CUDA 可见度、标记、权重与拟合参数。差异按原输入 RMS 归一化，阈值为 `1e-5`。

```bash
python scripts/compare_21cma_noncal.py /path/to/noncal_cpu /path/to/noncal_cuda \
  --output /path/to/comparison.json
```

## 仍保留的边界

本轮确认的是单 XX、MS 输入输出与 CPU/CUDA 流程。FEKO/NEC 波束物理模型仍需针对观测评估。
完整逐源逐天线 Jones peeling、21CMA UVFITS 写出、MWA 专属 raw/RTS 数据路线、HIP 硬件与完整
24 小时生产成像未完成或未验证；具体说明见功能指南。现有电离层 peeling 可用，不能等同于完整 Jones peeling。
