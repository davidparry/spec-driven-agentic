## Workshop

Step 1

```text
ollama pull qwen3.8-flash-next:125b-mlx
```

Step 2

```text
scripts/preflight.sh
```

Step 3

```text
git checkout -b workshop-spec trunk
```

Step 4

```text
spec model use qwen3.8-flash-next:125b-mlx
```

Step 5

```text
spec validate
```

Step 6

```text
spec list
```

Step 7

```text
spec test
```

Step 8

```text
spec draft
```

```text
Add a new requirement to requirements/requirements.json: a custom delimiter may be declared on the first line, so "//+\n1+2" adds up to 3. Follow the existing format — unique id, title, user story, acceptance criteria phrased Given/When/Then, status pending. Then call validate_spec and fix every issue until the spec is valid. Then call refine_requirement on the new requirement and reword it from the findings until there are none. Do not write scenarios or code yet — we are only agreeing on the spec.
```

Step 9

```text
spec refine REQ-007
```

Step 10

```text
spec list
```

Step 11

```text
spec show REQ-003
```

Step 12

```text
spec scenario generate REQ-003
```

Step 13

```text
spec steps missing
```

Step 14

```text
spec unittest generate REQ-003
```

Step 15

```text
spec test
```

Step 16

```text
spec implement REQ-003
```

Step 17

```text
spec test
```

Step 18

```text
spec refactor --note "extract comma delimiter constant" --req REQ-003
```

Step 19

```text
git diff
```

Step 20

```text
spec test
```

Step 21

```text
spec mark-implemented REQ-003
```

Step 22

```text
scripts/verify-workshop-run.sh check
```

## Homework

Step 23

```text
spec show REQ-004
```

Step 24

```text
spec scenario generate REQ-004
```

Step 25

```text
spec steps missing
```

Step 26

```text
spec unittest generate REQ-004
```

Step 27

```text
spec test
```

Step 28

```text
spec implement REQ-004
```

Step 29

```text
spec test
```

Step 30

```text
spec refactor --note "<what>" --req REQ-004
```

Step 31

```text
git diff
```

Step 32

```text
spec test
```

Step 33

```text
spec mark-implemented REQ-004
```

Step 34

```text
spec show REQ-005
```

Step 35

```text
spec scenario generate REQ-005
```

Step 36

```text
spec steps missing
```

Step 37

```text
spec unittest generate REQ-005
```

Step 38

```text
spec test
```

Step 39

```text
spec implement REQ-005
```

Step 40

```text
spec test
```

Step 41

```text
spec refactor --note "<what>" --req REQ-005
```

Step 42

```text
git diff
```

Step 43

```text
spec test
```

Step 44

```text
spec mark-implemented REQ-005
```

Step 45

```text
spec show REQ-006
```

Step 46

```text
spec scenario generate REQ-006
```

Step 47

```text
spec steps missing
```

Step 48

```text
spec unittest generate REQ-006
```

Step 49

```text
spec test
```

Step 50

```text
spec implement REQ-006
```

Step 51

```text
spec test
```

Step 52

```text
spec refactor --note "<what>" --req REQ-006
```

Step 53

```text
git diff
```

Step 54

```text
spec test
```

Step 55

```text
spec mark-implemented REQ-006
```

Step 56

```text
spec show REQ-007
```

Step 57

```text
spec scenario generate REQ-007
```

Step 58

```text
spec steps missing
```

Step 59

```text
spec steps generate
```

Step 60

```text
spec unittest generate REQ-007
```

Step 61

```text
spec test
```

Step 62

```text
spec implement REQ-007
```

Step 63

```text
spec test
```

Step 64

```text
spec refactor --note "<what>" --req REQ-007
```

Step 65

```text
git diff
```

Step 66

```text
spec test
```

Step 67

```text
spec mark-implemented REQ-007
```

Step 68

```text
spec list
```

Step 69

```text
spec validate
```

Step 70

```text
spec test
```

Step 71

```text
mvn -f kata/pom.xml test
```

## Optional — the decision model (Extra F)

Needs Ollama 0.35 or newer, and a `spec` built from this repository
(`cargo install --path harness`) — `spec judge` landed after `v0.7.0`
was tagged and is in no published binary yet. Nothing above depends on
it.

Step 72

```text
ollama pull nimble
```

Step 73

```text
spec judge models
```

Step 74

```text
spec judge use nimble:latest
```

Step 75

```text
spec config | grep -E "llm.model|decision.model"
```

Step 76

```text
spec judge criterion --text "Given the refactored module, when the suite runs, then code quality is improved by at least 20%"
```

Step 77

```text
spec judge criterion --text "Given a production-grade request payload, when the handler executes, then the system achieves 99.9% correctness across all code paths"
```

Step 78

```text
spec refine REQ-007
```

Step 79

```text
git checkout -- .spec/config.toml
```
