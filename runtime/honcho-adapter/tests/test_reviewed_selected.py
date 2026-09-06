"""Selected production HTTP recall must filter before the finite result window."""

from uuid import UUID, uuid4

from .test_reviewed_records import audience, base, body, mutation, record_url, stored
from .conftest import bundle, counts


async def test_reviewed_selected_applies_current_ids_before_limit_and_never_writes(client):
    owner, project = await bundle(client), uuid4()
    # The permitted record sorts after more than a full unfiltered result page.
    ids = [UUID(int=index) for index in range(1, 11)]
    for record in ids:
        result = await client.post(
            record_url(owner, project, record), json=body(owner, "deployment fact")
        )
        assert result.status_code == 201
    before = await stored(owner)
    request = {**audience(owner), "query": "deployment", "record_ids": [str(ids[-1])]}
    result = await client.post(base(owner, project) + "/recall-selected", json=request)
    assert result.status_code == 200
    assert [item["record_id"] for item in result.json()["records"]] == [str(ids[-1])]
    assert not result.json()["truncated"]
    assert await stored(owner) == before
    assert await counts(owner) == (0, 0, 0, 0)
    for invalid in ([], [str(ids[-1]), str(ids[-1])], [str(UUID(int=0))],
                    [str(uuid4()) for _ in range(33)]):
        assert (await client.post(base(owner, project) + "/recall-selected",
                json={**request, "record_ids": invalid})).status_code == 422
    assert (await client.post(base(owner, project) + "/recall-selected",
            json={**request, "company_id": str(uuid4())})).status_code == 409
    assert (await client.post(base(owner, uuid4()) + "/recall-selected",
            json=request)).json()["records"] == []
    assert (await client.post(record_url(owner, project, ids[-1], "withdraw"),
            json=mutation(owner))).status_code == 200
    assert (await client.post(base(owner, project) + "/recall-selected",
            json=request)).json() == {"records": [], "truncated": False}


async def test_reviewed_selected_keeps_record_and_utf8_budgets(client):
    owner, project = await bundle(client), uuid4()
    ids = [uuid4() for _ in range(9)]
    for record in ids:
        assert (await client.post(record_url(owner, project, record),
                json=body(owner, "deployment " * 150))).status_code == 201
    result = await client.post(base(owner, project) + "/recall-selected",
        json={**audience(owner), "query": "deployment", "record_ids": list(map(str, ids))})
    assert result.status_code == 200
    assert len(result.json()["records"]) == 4 and result.json()["truncated"]
    assert sum(len(item["content"].encode()) for item in result.json()["records"]) == 6600
