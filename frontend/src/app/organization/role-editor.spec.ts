import { TestBed } from '@angular/core/testing';
import { RoleEditor } from './role-editor';

describe('Role editor draft', () => {
  it('keeps organization, project, and role permissions separate and resets on organization change', async () => {
    TestBed.configureTestingModule({ imports: [RoleEditor] });
    const fixture = TestBed.createComponent(RoleEditor);
    fixture.componentRef.setInput('organizationId', '1');
    fixture.componentRef.setInput('projects', [
      { id: 1, org_id: 1, created_by_user_id: 1, name: 'Training' },
      { id: 2, org_id: 1, created_by_user_id: 1, name: 'Evaluation' },
    ]);
    fixture.componentRef.setInput('members', [
      { id: 1, username: 'alice', role_id: null, role_name: null },
    ]);
    fixture.detectChanges();
    await fixture.whenStable();
    const page: HTMLElement = fixture.nativeElement;
    const addRole = async (name: string) => {
      const input = page.querySelector<HTMLInputElement>('#new-role-name')!;
      input.value = name;
      input.dispatchEvent(new Event('input'));
      await fixture.whenStable();
      Array.from(page.querySelectorAll('button')).find((button) => button.textContent === 'Add role')!.click();
      await fixture.whenStable();
    };
    await addRole('Researcher');
    const checkbox = page.querySelector<HTMLInputElement>('[aria-label="Edit project for Training"]')!;
    checkbox.click();
    page.querySelector<HTMLInputElement>('fieldset input')!.click();
    await fixture.whenStable();
    expect(checkbox.checked).toBe(true);
    expect(page.querySelector<HTMLInputElement>('[aria-label="Edit project for Evaluation"]')!.checked).toBe(false);
    expect(page.querySelector<HTMLInputElement>('[aria-label="Start deployments for Training"]')!.checked).toBe(false);

    await addRole('Observer');
    expect(page.querySelector<HTMLInputElement>('[aria-label="Edit project for Training"]')!.checked).toBe(false);
    expect(page.querySelector<HTMLInputElement>('fieldset input')!.checked).toBe(false);
    Array.from(page.querySelectorAll('.role-list li button')).find((button) => button.textContent === 'Researcher')!
      .dispatchEvent(new MouseEvent('click'));
    await fixture.whenStable();
    expect(page.querySelector<HTMLInputElement>('[aria-label="Edit project for Training"]')!.checked).toBe(true);
    expect(page.querySelector<HTMLInputElement>('fieldset input')!.checked).toBe(true);

    const assignment = page.querySelector<HTMLSelectElement>('#member-role-1')!;
    assignment.value = '1';
    assignment.dispatchEvent(new Event('change'));
    await fixture.whenStable();
    expect(assignment.value).toBe('1');
    fixture.componentRef.setInput('organizationId', '2');
    await fixture.whenStable();
    expect(page.querySelectorAll('.role-list li button').length).toBe(0);
    expect(assignment.value).toBe('');
  });
});
