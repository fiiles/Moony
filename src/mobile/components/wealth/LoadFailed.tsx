/**
 * A class page whose list could not be read (Akcie, Krypto, Dluhopisy, Ostatní aktiva): one
 * centred sentence in place of the rows, announced as an alert. `children` is the class's own
 * sentence ("Akcie se nepodařilo načíst.").
 */
export function LoadFailed({ children }: { children: string }) {
  return (
    <p role="alert" className="mx-0.5 py-6 text-center text-m-body text-ink-3">
      {children}
    </p>
  );
}
