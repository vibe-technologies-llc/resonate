from pathlib import Path
import sys
import xml.etree.ElementTree as ElementTree

results = Path(__file__).resolve().parent / "app/build/outputs/androidTest-results/connected"
failures = 0
for report in sorted(results.glob("**/TEST-*.xml")):
    for case in ElementTree.parse(report).getroot().iter("testcase"):
        for problem in [*case.findall("failure"), *case.findall("error")]:
            failures += 1
            print(f"FAILED {case.get('classname')}.{case.get('name')} ({case.get('time')} s) on {report.parent.name}")
            print(problem.text or problem.get("message") or "")
if failures:
    sys.exit(1)
