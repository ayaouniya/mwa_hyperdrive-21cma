# 21CMA 非校准功能验证 — 2026-09-09

模拟、减源、CPU/CUDA 电离层 peeling、MS 转换/平均、波束选源、源表工具、波束导出，以及解文件转换/绘图均完成本机验证。使用方式与限制见 [NON_CALIBRATION.md](NON_CALIBRATION.md)，数值、文件指纹和各命令耗时见 [机器可读记录](validation_20260909.json)。

## 输入和可复现性

- 已校准观测：`/data/zfy/21cma_hyperdrive_project/imaging_runs/b07_structure_mitigation_20260720/vis/b07_cal_pilot_t0700_0799_tavg2_favg2_ch001_120.ms`。
- 天空模型：`/home/zhaofyastro/21cma_hyperdrive_project/models/21cma_ncp_hybrid_v1_3c61lotssgaus_j235522lotss_core5_100mjy_v1.yaml`，含 1784 个源、1848 个分量。验证选择波束排序后的前三个源，共 35 个点源/高斯分量。
- FEKO 波束：`/data/zfy/21cma_hyperdrive_project/beam_feko/processed/cma21_feko_formed_50_200mhz_nativefreq_halfsky_0p25_steering_shapeonly_centernorm_v1.h5`。
- 子集：原 MS 第 `0,1,2,3,10,11,12` 个时间，40 个天线、780 条互相关基线、5460 行、120 个频道，138.134765625–149.755859375 MHz。输出为原生单 XX。
- peeling：3 passes、10 loops、前两个源电离层拟合；时间平均因子 3、频率平均因子 7、`--uvw-min 0m`。三个时间块包含 3、3、1 个采样；末尾频率块只有 1 个频道。
- CPU 与 CUDA 使用同一 release 二进制，CPU 用 `--cpu`。CUDA 在 GPU 1（A100 80GB，compute 80）运行；Rust 1.87.0、4 个 Rayon 线程。二进制 SHA-256 记录在 JSON 中；构建时版本日志显示基线 `5633c54 (dirty)`，实现随本报告提交。

本机已验证的二进制：

```text
/data/zfy/21cma_hyperdrive_project/noncal_validation_20260909/build-cuda/release/hyperdrive
```

可复现脚本：[`scripts/validate_21cma_noncal.py`](../../scripts/validate_21cma_noncal.py)。每次要求新的输出目录，输入 MS 只读。完整 `commands.json`、`report.json`、MS、拟合 JSON、波束表、源表和逐命令日志保存在：

```text
/data/zfy/21cma_hyperdrive_project/noncal_validation_20260909/cpu_final/
/data/zfy/21cma_hyperdrive_project/noncal_validation_20260909/cuda_final/
```

原观测和大型生成文件不纳入 Git。带 `--solutions /data/zfy/21cma_hyperdrive_project/tmp_bandtest_125_150_solutions.fits` 的 CUDA 验证还执行了 FITS→AO bin→FITS→AO bin，核对维度及 Jones 数据逐字节一致，并生成 3 个时间块的 6 张幅度/相位 PNG。

## 数值结果

RMS 定义为可见度复数模平方平均的平方根。真实观测指标仅使用未标记样本；注入测试使用完整纯模型网格。

| 检查 | CPU | CUDA |
| --- | ---: | ---: |
| `data - model` 相减闭合误差 / 输入 RMS | 1.963×10⁻⁸ | 1.966×10⁻⁸ |
| 注入测试剩余 RMS / 注入误差 RMS | 1.843×10⁻⁶ | 1.836×10⁻⁶ |
| 保留非拟合源的闭合误差 RMS（Jy） | 9.567×10⁻⁸ | 2.220×10⁻⁹ |
| 真实输入 RMS（Jy） | 20.306879 | 20.306879 |
| 真实输入直接减源后的 RMS（Jy） | 16.331575 | 16.331575 |
| 真实输入 peeling 后的 RMS（Jy） | 16.304798 | 16.304798 |

注入测试为 `D = M_all + 0.1 M_3C390.3`，只拟合最亮源 3C390.3，检查输出接近零、增益恢复到 1.1。CPU 三块增益与 1.1 的最大偏差小于 9×10⁻¹⁰，CUDA 小于 5.3×10⁻⁸。位移参数接近零，绝对值小于 1.6×10⁻¹¹。

CPU/CUDA 模型、直接减源、真实 peeling 的相对差异 RMS 分别为 `4.918×10⁻⁸`、`4.080×10⁻⁸`、`1.247×10⁻⁷`。输出的时间、分数 Hz 频率、XX 相关积形状、望远镜名、原始标记和有效权重均通过独立 casacore 读取验证。

这次实测发现并修复了扩展源复数拟合与 CUDA 高斯源坐标系问题。修复前同样的 GPU 增益注入检查留下约 8% 的注入误差；修复后约 0.00018%。FEKO 表共享存储也消除了重复大表复制，本机同一小样本真实 peeling 从初测约 97 秒降到约 6.5 秒。耗时含加载与 I/O，机器同时有其他任务，不能当作通用 CPU/GPU 性能基准。

## 自动回归覆盖

最终结果：CPU 库测试 **316/316**，CUDA 库测试 **373/373**，CLI 集成测试 **24/24**；CPU 库及主程序/示例/集成测试/benchmark 的严格 Clippy 检查通过，`cargo fmt --all --check` 与 `git diff --check` 通过。测试命令和日志路径记录在机器可读报告的 `tests` 中。

测试使用 `MWA_BEAM_FILE=/home/zhaofyastro/MWA/mwa_full_embedded_element_pattern.h5`、`RAYON_NUM_THREADS=4`、`--test-threads=1`；CUDA 构建加 `HYPERDRIVE_CUDA_COMPUTE=80 HYPERBEAM_CUDA_COMPUTE=80 CUDA_PATH=/usr/local/cuda CUDA_VISIBLE_DEVICES=1`。主要覆盖：

- CPU 与 CUDA 库测试，包括既有校准、模型、波束、单源/多源 peeling 回归。
- 新增原生 XX 小型 MS 的模拟→相减→peeling 闭合、高斯扩展源、自相关透传、精确时间/频率和选源。
- 不规则时间间隔、末尾单独时间块、10 个频道按 6+4 平均、全标记/NaN 样本与不合法模拟参数。
- 复数扩展源亮度误差不会误拟合为位置偏移；CPU XX 注入恢复。
- 多时间块解的 AO/FITS 往返可读性、Jones 数值和标记保留。

## 验证边界与暂缓项

上述真实数据结果证明处理链可运行和数值闭合。残差 RMS 下降本身不能证明校准或成像在科学上更准确，仍需针对目标观测检查图像、模型完整性和长期稳定性。

完整逐源逐天线 Jones peeling、显式 21CMA UVFITS 写出、MWA 专属原始数据/RTS 操作、HIP 硬件和完整 24 小时生产任务未在本轮实现或验证，具体原因见功能指南。真实样本使用点源和高斯源；shapelet 仅有通用建模器回归。AO `.bin` 的格式限制会丢失逐块时间及其他 FITS 元数据，应保留原始 FITS 用于科学处理。
