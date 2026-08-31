"""Generated CompanyOS public API client — companyos-python-sdk-gen@1.0.0."""
from __future__ import annotations
import urllib.request
import json
from typing import Any, Optional

class CompanyOsPublicClient:
    def __init__(self, base_url: str, api_key: str, timeout: float = 30.0):
        self.base_url = base_url.rstrip("/")
        self.api_key = api_key
        self.timeout = timeout

    def _request(self, method: str, path: str, body: Optional[dict[str, Any]] = None, idempotency_key: Optional[str] = None) -> Any:
        data = None if body is None else json.dumps(body).encode("utf-8")
        headers = {
            "Authorization": f"Bearer {self.api_key}",
            "Accept": "application/json",
            "Content-Type": "application/json",
        }
        if idempotency_key:
            headers["Idempotency-Key"] = idempotency_key
        req = urllib.request.Request(self.base_url + path, data=data, headers=headers, method=method)
        with urllib.request.urlopen(req, timeout=self.timeout) as resp:
            raw = resp.read().decode("utf-8")
            return json.loads(raw) if raw else None

    def list_customers(self) -> Any:
        return self._request("GET", "/api/v1/sales/customers")

    def create_customer(self, body: dict[str, Any], idempotency_key: str) -> Any:
        return self._request("POST", "/api/v1/sales/customers", body, idempotency_key)

    def list_invoices(self) -> Any:
        return self._request("GET", "/api/v1/finance/invoices")

    def create_invoice(self, body: dict[str, Any], idempotency_key: str) -> Any:
        return self._request("POST", "/api/v1/finance/invoices", body, idempotency_key)

