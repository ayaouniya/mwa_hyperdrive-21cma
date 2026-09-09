# 21CMA 非校准功能：用法与验证

本页记录 2026-09-09 对非校准功能的开发和检查。21CMA 的科学数据按实际测量的 **XX** 处理；输入可以是原生单 XX MS，也可以是旧版将 XX 装在四相关容器中的 MS。

## 功能范围

| 功能 | 21CMA 入口及边界 |
| --- | --- |
| `vis-convert` | 支持选择时间、时间/频率平均和 MS 输出，保留实际时间质心、分数 Hz 频率与单 XX 元数据。 |
| `vis-simulate` | 新增 `--data template.ms --telescope 21cma`，从 MS 取得阵列、相位中心、频道和时间。支持点源、高斯源、已有 shapelet 建模器及 `--output-autos`；真实数据验证使用点源和高斯源。 |
| `vis-subtract` | 支持在 21CMA 数据上直接减去天空模型，使用显式选择的 21CMA 波束。 |
| `peel` | CPU/CUDA 均支持 XX 电离层位移与标量亮度拟合、全模型相减、保留非拟合源、时间/频率平均及自相关透传。 |
| `srclist-by-beam` | 新增 `--data observation.ms --telescope 21cma`，按真实观测信息及指定波束筛选源，不再需要构造 MWA metafits。 |
| `srclist-verify/convert/shift` | 可以直接处理 21CMA 天空模型；普通格式转换与位置偏移不需要望远镜元数据。 |
| `beam` | 可导出 `cma21-gaussian` / `cma21-feko-cube` 波束响应；高斯波束须传入实际阵列纬度。输出是命令定义的 Jones 幅度代理量，不是经科学归一化的 XX 功率束。 |
| `solutions-convert/plot` | 支持已有 21CMA 解文件的格式转换及绘图；绘图需要默认 `plotting` feature。未测量的相关积不能解释为科学测量；AO `.bin` 不保留全部 FITS 元数据。 |
| `solutions-apply` | 已有能力，继续使用 `--telescope 21cma`；本轮同时运行相关回归测试。 |

显式 21CMA 路径的默认可见度输出现在使用 `.ms`，`OBSERVATION/TELESCOPE_NAME` 写为 `21CMA`。

## 模拟、减源和 peeling

下面的路径是需要替换的示例。peeling 和模型相减应使用已校准的输入，或者通过已有输入参数传入合适的校准解。

```bash
# 从实际观测网格生成纯模型
hyperdrive vis-simulate \
  --data calibrated.ms --telescope 21cma \
  --source-list sky.yaml \
  --beam-type cma21-feko-cube --beam-file beam.h5 \
  --output-model-files model.ms

# 直接减去源模型
hyperdrive vis-subtract \
  --data calibrated.ms --telescope 21cma \
  --source-list sky.yaml \
  --beam-type cma21-feko-cube --beam-file beam.h5 \
  --outputs residual.ms

# 在前三个亮源构成的模型中拟合前两个源的电离层参数
hyperdrive peel \
  --data calibrated.ms --telescope 21cma \
  --source-list sky.yaml --num-sources 3 --iono-sub 2 \
  --beam-type cma21-feko-cube --beam-file beam.h5 \
  --iono-time-average 3 --iono-freq-average 7 \
  --num-passes 3 --num-loops 10 \
  --outputs peeled.ms iono.json
```

- `--iono-sub` 作用于波束筛选、排序后的源。检查日志和输出 JSON 中的源名；它不是输入 YAML 行号。
- 默认输出减去整个所选天空模型。若只希望移除拟合的源，加 `--preserve-non-iono-sources`，其余源只参与构造拟合残差。
- `--autos` 会在输入包含自相关时保留它们；peeling 只修正互相关，自相关仅经历请求的输入处理和输出平均。
- `--iono-xx-only` 在显式 21CMA 路径已自动启用，无需重复指定。
- `--iono-time-average 3` 按顺序组合三个读入时间块；最后不足三个也可处理。跨很长间隔组合数据会假设电离层参数在这些采样之间不变，应根据科学需求选择时间区间。
- `--iono-freq-average 7` 可处理不能被 7 整除的频道数，无需裁掉最后几个频道。最后一块使用实际包含频道的中心频率。
- JSON 的 `centroid_timestamps_gps_seconds` 与每个源的 `alphas`、`betas`、`gains` 一一对应。它表示 **GPS 秒**，不是 UTC Unix 秒。
- 可在模拟、相减和 peeling 命令中加 `--cpu` 强制 CPU。CUDA 模式取决于编译 feature，可用 `CUDA_VISIBLE_DEVICES` 选择 GPU。

模拟时，MS 的可见度值和逐样本 FLAG/WEIGHT **不复制到纯模型**。纯模型使用模拟权重；不可将它当作原始数据的权重模板。输入中不可用的天线仍被排除。默认使用全部时间（包括标记时间），也可以用 `--timesteps` 选择。

MS 模板已经定义相位中心、频率和时间，因此不能再与 `--metafits`、`--ra/--dec`、`--num-fine-channels`、`--freq-res`、`--middle-freq`、`--num-timesteps`、`--time-res` 或 `--time-offset` 混用；冲突会明确报错。可使用 `--output-model-time-average` 和 `--output-model-freq-average` 平均输出。`--freq-res` 的旧 metafits 模式单位仍是 kHz。

## 根据波束选择源

```bash
hyperdrive srclist-by-beam sky.yaml brightest.json \
  --data calibrated.ms --telescope 21cma \
  --beam-type cma21-feko-cube --beam-file beam.h5 \
  --number 10

hyperdrive beam cma21-gaussian --freq-mhz 150 \
  --latitude-deg 42.9242 --output gaussian.tsv
```

筛选使用输入的第一个选中观测时刻。对长时间段，应分时段检查波束筛选和模型覆盖情况。

## 这次修复的关键问题

1. **时间块**：peeling 不再将 21CMA 时间戳重新套进规则时间网格。21CMA 时间平均不再继承通用旧函数的 10 ms 取整。
2. **频率块**：显式传递平均因子，避免通过输入/输出长度反推错误因子；CPU/GPU 正确处理尾部不足整块的频道，GPU 不越界读取。
3. **扩展源拟合**：采用复数最小二乘中的 `Im(conj(M) D)`、`Re(conj(M) D)` 和 `|M|²`，避免将模型虚部造成的亮度误差混进位置拟合。纯实点源情形仍与原式一致。
4. **CUDA 高斯源**：先在原观测坐标系计算模型，再旋转到源方向，保持高斯形状所用 UV 坐标与初始模型一致。
5. **标记和求解失败**：平均时跳过零权重样本，空块清零；非有限拟合更新保留上一组参数并记录警告。该保护不代表该块成功测得电离层。
6. **自相关与元数据**：自相关经过处理链完整透传；MS 记录真实的 21CMA 望远镜名和单 XX 相关形状。
7. **FEKO 内存复制**：不可变波束立方体使用共享存储，避免 GPU 对每个源/时间重新复制大型表。
8. **解格式往返**：AO `.bin` 只保存整段起止时间。多时间块转回 FITS 时，省略不完整的逐块时间列并发出警告，避免产生无法读回的 FITS。Jones 数值与标记保留；不规则观测应保留原始 FITS，不能通过 `.bin` 往返恢复逐块时间、频率和天线元数据。

## 可复现的实测检查

仓库提供 `scripts/validate_21cma_noncal.py`，依赖 `numpy` 和 `python-casacore`。它只读取原始输入，在全新的输出目录建立小型子集、保存每条命令与日志，并在断言通过后生成 `report.json`。

```bash
CUDA_VISIBLE_DEVICES=1 RAYON_NUM_THREADS=4 \
python3 scripts/validate_21cma_noncal.py \
  --hyperdrive /path/to/hyperdrive \
  --input-ms /path/to/calibrated.ms \
  --source-list /path/to/sky.yaml \
  --beam-file /path/to/beam.h5 \
  --output-dir /path/to/new-validation-directory
```

加 `--cpu` 可对同一 CUDA 编译版本的 CPU 路径运行相同检查。可选 `--solutions small_solutions.fits` 还会检查解格式转换和绘图。

检查内容包括：MS 时间选择、单 XX 元数据、频率、权重和标记保留，`data - model` 数值闭合，真实数据 peeling、在真实阵列网格上注入 10% 亮源增益后的恢复、保留非拟合源的闭合、输出平均、源表工具和波束导出。小型 Rust 回归还检查不规则时间、最后一个时间/频道块、自相关、扩展源，以及不合法的模板参数。

本机实测结果和验证范围见 [VALIDATION_20260909.md](VALIDATION_20260909.md)。真实观测上的残差变化与注入恢复会分别报告；小型闭合测试不能替代全观测科学成像评估。

## 本轮保留的限制

- **完整的逐源、逐天线 Jones peeling**：上游目前也只实现电离层相减，完整逐源增益求解尚未接通；本轮不把 `--iono-sub` 描述成完整 DDE 校准。参见[上游说明](https://mwatelescope.github.io/mwa_hyperdrive/user/peel/intro.html)。
- **显式 21CMA UVFITS 输出**：当前写出路径只为 MS 保证不规则时间语义，继续明确拒绝 UVFITS。这里不建议用 `--telescope standard` 绕过检查。
- **MWA 原始 gpubox、dipole-gains、RTS 专属 metafits/patch 操作**：属于 MWA 仪器接口，不适用于 21CMA MS；21CMA 上游原始相关数据转 MS 不在 Hyperdrive 内实现。
- **波束物理与科学验证**：高斯束是近似，FEKO 束是当前离线标量功率模型，不能代表完整逐 pod 复 Jones、交叉极化或时间变化。未验证 HIP 硬件、完整 24 小时生产 peeling 或最终科学功率谱/图像改善。
