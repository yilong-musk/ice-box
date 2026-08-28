import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ErrorAlert, OkAlert, WarnAlert } from "./StatusAlert";

describe("StatusAlert", () => {
  it("exposes error, warning, and success as alerts by default", () => {
    render(
      <>
        <ErrorAlert>load failed</ErrorAlert>
        <WarnAlert>proxy desync</WarnAlert>
        <OkAlert>saved</OkAlert>
      </>,
    );

    const alerts = screen.getAllByRole("alert");
    expect(alerts).toHaveLength(3);
    expect(alerts[0]).toHaveTextContent("load failed");
    expect(alerts[1]).toHaveTextContent("proxy desync");
    expect(alerts[2]).toHaveTextContent("saved");
  });

  it("keeps role=alert even when the caller omits role", () => {
    render(<WarnAlert>proxy desync</WarnAlert>);
    expect(screen.getByRole("alert")).toHaveTextContent("proxy desync");
  });
});
