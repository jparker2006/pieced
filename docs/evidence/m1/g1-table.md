| Run | Scenario | Commit | Build | Power | Low Power Mode | Battery | Graphics | Conditions |
|---|---|---|---|---|---|---|---|---|
| gate-20260925-013236-launch1 | smoke | b916e218aaea | release | Battery | on | 22% | Battery preset, render scale 1.00, vsync true, cap 60 | - |
| gate-20260925-013236-launch2 | smoke | b916e218aaea | release | Battery | on | 22% | Battery preset, render scale 1.00, vsync true, cap 60 | - |
| gate-20260925-013236-launch3 | smoke | b916e218aaea | release | Battery | on | 22% | Battery preset, render scale 1.00, vsync true, cap 60 | - |

| Gate | Run | Measurement | Threshold | Result |
|---|---|---|---|---|
| G1 | gate-20260925-013236-launch1 | launch 2060 ms (release) | < 5000 ms | PASS |
| G1 | gate-20260925-013236-launch2 | launch 711 ms (release) | < 5000 ms | PASS |
| G1 | gate-20260925-013236-launch3 | launch 684 ms (release) | < 5000 ms | PASS |

G1 over the last 3 launches listed (2060, 711, 684 ms): PASS
