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
spec changes show
```

Step 10

```text
spec changes commit
```

Step 11

```text
spec refine REQ-007
```

Step 12

```text
spec list
```

Step 13

```text
spec show REQ-003
```

Step 14

```text
spec scenario generate REQ-003
```

Step 15

```text
spec steps missing
```

Step 16

```text
spec unittest generate REQ-003
```

Step 17

```text
spec changes show
```

Step 18

```text
spec changes commit
```

Step 19

```text
spec test
```

Step 20

```text
spec implement REQ-003
```

Step 21

```text
spec changes show
```

Step 22

```text
spec changes commit && spec test
```

Step 23

```text
spec refactor --note "extract comma delimiter constant" --req REQ-003
```

Step 24

```text
git diff
```

Step 25

```text
spec test
```

Step 26

```text
spec mark-implemented REQ-003
```

Step 27

```text
spec changes commit
```

Step 28

```text
scripts/verify-workshop-run.sh check
```

## Homework

Step 29

```text
spec show REQ-004
```

Step 30

```text
spec scenario generate REQ-004
```

Step 31

```text
spec steps missing
```

Step 32

```text
spec unittest generate REQ-004
```

Step 33

```text
spec changes show
```

Step 34

```text
spec changes commit
```

Step 35

```text
spec test
```

Step 36

```text
spec implement REQ-004
```

Step 37

```text
spec changes show
```

Step 38

```text
spec changes commit && spec test
```

Step 39

```text
spec refactor --note "<what>" --req REQ-004
```

Step 40

```text
git diff
```

Step 41

```text
spec test
```

Step 42

```text
spec mark-implemented REQ-004
```

Step 43

```text
spec changes commit
```

Step 44

```text
spec show REQ-005
```

Step 45

```text
spec scenario generate REQ-005
```

Step 46

```text
spec steps missing
```

Step 47

```text
spec unittest generate REQ-005
```

Step 48

```text
spec changes show
```

Step 49

```text
spec changes commit
```

Step 50

```text
spec test
```

Step 51

```text
spec implement REQ-005
```

Step 52

```text
spec changes show
```

Step 53

```text
spec changes commit && spec test
```

Step 54

```text
spec refactor --note "<what>" --req REQ-005
```

Step 55

```text
git diff
```

Step 56

```text
spec test
```

Step 57

```text
spec mark-implemented REQ-005
```

Step 58

```text
spec changes commit
```

Step 59

```text
spec show REQ-006
```

Step 60

```text
spec scenario generate REQ-006
```

Step 61

```text
spec steps missing
```

Step 62

```text
spec unittest generate REQ-006
```

Step 63

```text
spec changes show
```

Step 64

```text
spec changes commit
```

Step 65

```text
spec test
```

Step 66

```text
spec implement REQ-006
```

Step 67

```text
spec changes show
```

Step 68

```text
spec changes commit && spec test
```

Step 69

```text
spec refactor --note "<what>" --req REQ-006
```

Step 70

```text
git diff
```

Step 71

```text
spec test
```

Step 72

```text
spec mark-implemented REQ-006
```

Step 73

```text
spec changes commit
```

Step 74

```text
spec show REQ-007
```

Step 75

```text
spec scenario generate REQ-007
```

Step 76

```text
spec steps missing
```

Step 77

```text
spec steps generate
```

Step 78

```text
spec unittest generate REQ-007
```

Step 79

```text
spec changes show
```

Step 80

```text
spec changes commit
```

Step 81

```text
spec test
```

Step 82

```text
spec implement REQ-007
```

Step 83

```text
spec changes show
```

Step 84

```text
spec changes commit && spec test
```

Step 85

```text
spec refactor --note "<what>" --req REQ-007
```

Step 86

```text
git diff
```

Step 87

```text
spec test
```

Step 88

```text
spec mark-implemented REQ-007
```

Step 89

```text
spec changes commit
```

Step 90

```text
spec list
```

Step 91

```text
spec validate
```

Step 92

```text
spec test
```

Step 93

```text
mvn -f kata/pom.xml test
```
