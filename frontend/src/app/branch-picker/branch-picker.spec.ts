import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';
import { BranchPicker } from './branch-picker';

const repository = { id: 42, full_name: 'Org/Repo', web_url: 'https://github.com/Org/Repo' };
const branches = [
  { name: 'main', commit_sha: 'a'.repeat(40) },
  { name: 'feature/Model', commit_sha: 'b'.repeat(40) },
];

describe('Branch picker', () => {
  beforeEach(() => {
    sessionStorage.setItem('authToken', 'saved-token');
    TestBed.configureTestingModule({ imports: [BranchPicker], providers: [provideHttpClient(), provideHttpClientTesting(), provideRouter([])] });
  });
  afterEach(() => { TestBed.inject(HttpTestingController).verify(); sessionStorage.clear(); });

  function create(provider: 'github' | 'gitlab' = 'github', selectedRepository: typeof repository | null = repository) {
    const fixture = TestBed.createComponent(BranchPicker);
    fixture.componentRef.setInput('provider', provider);
    fixture.componentRef.setInput('repository', selectedRepository);
    fixture.detectChanges();
    return fixture;
  }

  function request(page = 1) {
    return TestBed.inject(HttpTestingController).expectOne(req => req.url.endsWith('/branches') && req.params.get('page') === String(page));
  }

  it('requires a repository before loading branches', () => {
    const fixture = create('github', null);
    expect(fixture.nativeElement.querySelector('.branch-trigger').disabled).toBe(true);
    TestBed.inject(HttpTestingController).expectNone(req => req.url.endsWith('/branches'));
  });

  it('searches all pages without case sensitivity and selects a branch with its head commit', async () => {
    const fixture = create();
    fixture.nativeElement.querySelector('.branch-trigger').click();
    const first = request();
    expect(first.request.params.get('repository')).toBe('Org/Repo');
    expect(first.request.headers.get('Authorization')).toBe('Bearer saved-token');
    first.flush({ branches: [branches[0]], has_more: true });
    request(2).flush({ branches: [branches[1]], has_more: false });
    await fixture.whenStable();
    const input = fixture.nativeElement.querySelector('input');
    input.value = '  FEATURE/model  ';
    input.dispatchEvent(new Event('input'));
    fixture.detectChanges();
    expect(fixture.nativeElement.querySelectorAll('[role="option"]').length).toBe(1);
    const selected = vi.fn();
    fixture.componentInstance.selectionChange.subscribe(selected);
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await fixture.whenStable();
    expect(selected).toHaveBeenCalledWith(branches[1]);
    expect(fixture.nativeElement.querySelector('.branch-trigger').textContent).toContain('feature/Model');
    expect(fixture.nativeElement.querySelector('.branch-dropdown')).toBeNull();
  });

  it('cancels stale pages and clears selection when the repository changes', async () => {
    const fixture = create('gitlab');
    fixture.nativeElement.querySelector('.branch-trigger').click();
    const first = request();
    expect(first.request.params.get('repository')).toBe('42');
    first.flush({ branches, has_more: true });
    const stale = request(2);
    fixture.detectChanges();
    fixture.nativeElement.querySelector('[role="option"]').click();
    const selected = vi.fn();
    fixture.componentInstance.selectionChange.subscribe(selected);
    fixture.componentRef.setInput('repository', { ...repository, id: 43 });
    fixture.detectChanges();
    expect(stale.cancelled).toBe(true);
    expect(selected).toHaveBeenCalledWith(null);
    expect(fixture.nativeElement.querySelector('.branch-trigger').textContent).toContain('Select branch');
    fixture.nativeElement.querySelector('.branch-trigger').click();
    request().flush({ branches: [], has_more: false });
    await fixture.whenStable();
    expect(fixture.nativeElement.textContent).toContain('No branches available.');
  });

  it('explains rejected branch access and allows retry', async () => {
    const fixture = create();
    fixture.nativeElement.querySelector('.branch-trigger').click();
    request().flush('Rejected', { status: 403, statusText: 'Forbidden' });
    await fixture.whenStable();
    expect(fixture.nativeElement.textContent).toContain('Check its permissions');
    fixture.nativeElement.querySelector('.branch-status button').click();
    request().flush({ branches, has_more: false });
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelectorAll('[role="option"]').length).toBe(2);
    fixture.nativeElement.querySelector('input').dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.branch-dropdown')).toBeNull();
  });
});
