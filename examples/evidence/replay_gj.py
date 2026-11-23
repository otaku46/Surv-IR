"""Replay a Surv evidence trace against gj's EvidenceStanding decision table."""

import argparse
import json
import sys
import tempfile
from pathlib import Path


def replay(trace, gj_path):
    sys.path.insert(0, str(gj_path.resolve()))
    from gj import cli, records
    from gj.store import Ledger

    if trace["version"] != 1:
        raise ValueError("unsupported replay version")

    with tempfile.TemporaryDirectory() as directory:
        ledger = Ledger.create(directory)
        objects = {}
        evidence_ids = {}
        verification_ids = {}
        scope = records.scope_declared_empty()
        initial = trace["initial"]
        direct_binding = "AttestationVerification" in records.FIELDS

        def put(record_type, **fields):
            payload = records.mk(record_type, **fields)
            oid = records.oid(record_type, payload)
            objects[oid] = (record_type, payload)
            return oid

        def subject(row, prefix, artifact):
            # The projection uses a synthetic subject per statement. Its
            # identity depends only on the statement and the artifact hash.
            statement = {"artifact": artifact}
            for field in ("attestor", "guarantee", "content"):
                statement[field] = row[prefix + field]
            anchor = records.oid("StatementTarget", statement)
            return records.subject("change_anchor", anchor=anchor)

        for symbol, row in initial["capsules"].items():
            artifact = ledger.put_artifact(row["artifact"].encode())
            target = (records.subject("change_anchor", anchor="a" * 64) if direct_binding
                      else subject(row, "target.", artifact))
            evidence_ids[symbol] = put(
                "EvidenceCapsule", target=target,
                claim=records.claim("Observation", "evidence"), scope=scope,
                premises="m" * 64, acquired_by="recorder", method="command",
                captured_at=1, artifact=artifact, support_strength="Direct",
            )

        rules = [
            records.mk("AttestorRule", principal=row["attestor"],
                       allowed_guarantees=row["guarantee"])
            for row in initial["rules"].values()
        ]
        ruleset = put("Ruleset", vocabulary_version="v", derivation_version="d",
                      providers=[], trusted_attestors=rules, reuse_rules=[])
        (Path(ledger.root) / "config.json").write_text(
            json.dumps({"Ruleset": ruleset}), encoding="utf-8"
        )

        def attest(artifact, attestor, guarantee, content, **links):
            return put(
                "Attestation", attestor=attestor, artifact=artifact,
                guarantee=guarantee, content=records.claim("Observation", content),
                captured_at=1, **links,
            )

        for symbol, row in initial["checks"].items():
            prefix = "obligation.target."
            artifact = ledger.put_artifact(row[prefix + "artifact"].encode())
            if direct_binding:
                target = attest(artifact, row[prefix + "attestor"],
                                row[prefix + "guarantee"], row[prefix + "content"])
                verification_ids[symbol] = put(
                    "AttestationVerification", attestation=target, ruleset=ruleset,
                    result=row["result"], observed_at=1,
                )
            else:
                links = ({"runner": "local-shell"}
                         if "runner" in records.FIELDS["VerificationRecord"] else {})
                verification_ids[symbol] = put(
                    "VerificationRecord", commit="c" * 64, environment="e" * 64,
                    criterion=artifact, result=row["result"], **links,
                    obligation={"requirement_version": "r", "template_key": "t",
                                "target": subject(row, prefix, artifact), "scope": scope},
                )

        def select(evidence, check, attestor, guarantee, content, artifact):
            verification = verification_ids[check]
            links = {} if direct_binding else {
                "verification": verification, "validation_ref": verification,
            }
            attestation = attest(artifact, attestor, guarantee, content, **links)
            put("EvidenceEvent", evidence=evidence, kind="AttestationAdded",
                recorder="recorder", observed_at=1, attestation=attestation)
            if direct_binding:
                put("EvidenceEvent", evidence=evidence, kind="VerificationRecorded",
                    recorder="recorder", observed_at=1, verification=verification)

        for row in initial["attestations"].values():
            # This acceptance model has one capsule and one attestation slot.
            evidence, = evidence_ids.values()
            select(evidence, row["validation_ref"], row["attestor"], row["guarantee"],
                   row["content"], ledger.put_artifact(row["artifact"].encode()))

        observations = []
        for step in trace["steps"]:
            args = step["args"]
            evidence = evidence_ids[args["evidence"]]
            if step["operation"] == "select_attestation":
                capsule = objects[evidence][1]
                select(evidence, args["verification"], args["attestor"], args["guarantee"],
                       args["content"], capsule["artifact"])
                standing = initial["standing"]
            elif step["operation"] == "evaluate_evidence":
                standing = cli._evidence_standing(ledger, objects, evidence)
            else:
                raise ValueError(f"unsupported operation {step['operation']!r}")
            observations.append({"standing": standing})
        return observations


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--gj", required=True, type=Path, help="gj checkout")
    parser.add_argument("--goal", help="select a named reach witness from a report")
    args = parser.parse_args()
    report = json.load(sys.stdin)
    if args.goal:
        trace = report["reached"][args.goal]
    elif "trace" in report:
        trace = report["trace"]["replay"]
    else:
        trace = report

    observations = replay(trace, args.gj)
    mismatches = []
    for index, (step, actual) in enumerate(zip(trace["steps"], observations), 1):
        expected = step["observations"]
        if expected is not None and any(actual[key] != value for key, value in expected.items()):
            mismatches.append({"step": index, "expected": expected, "actual": actual})
    print(json.dumps({"observations": observations, "mismatches": mismatches}, indent=2))
    return bool(mismatches)


if __name__ == "__main__":
    sys.exit(main())
