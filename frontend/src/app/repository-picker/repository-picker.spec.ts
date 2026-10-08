import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';
import { Router, provideRouter } from '@angular/router';
import { RepositoryPicker } from './repository-picker';

const repositories = [
  { id: 1, full_name: 'Research/Training', web_url: 'https://github.com/Research/Training' },
  { id: 2, full_name: 'Hospital/Model', web_url: 'https://github.com/Hospital/Model' },
];

describe('Repository picker', () => {
  beforeEach(() => {
    sessionStorage.setItem('authToken', 'saved-token');
    TestBed.configureTestingModule({
      imports: [RepositoryPicker],
      providers: [provideHttpClient(), provideHttpClientTesting(), provideRouter([])],
    });
  });
  afterEach(() => {
    TestBed.inject(HttpTestingController).verify();
    sessionStorage.clear();
  });

  function create(provider: 'github' | 'gitlab' = 'github') {
    const fixture = TestBed.createComponent(RepositoryPicker);
    fixture.componentRef.setInput('provider', provider);
    fixture.detectChanges();
    const http = TestBed.inject(HttpTestingController);
    http.expectNone((request) => request.url.startsWith('/connectors/'));
    fixture.nativeElement.querySelector('.repository-trigger').click();
    fixture.detectChanges();
    return fixture;
  }

  function search(fixture: ReturnType<typeof create>, value: string) {
    const input = fixture.nativeElement.querySelector('input');
    input.value = value;
    input.dispatchEvent(new Event('input'));
    fixture.detectChanges();
  }

  it('loads every page and searches namespace and repository names without case sensitivity', async () => {
    const fixture = create();
    const http = TestBed.inject(HttpTestingController);
    const first = http.expectOne('/connectors/github/repositories?page=1');
    expect(first.request.headers.get('Authorization')).toBe('Bearer saved-token');
    first.flush({ repositories: [repositories[0]], has_more: true });
    http.expectOne('/connectors/github/repositories?page=2').flush({ repositories: [repositories[1]], has_more: false });
    await fixture.whenStable();
    search(fixture, '  hOsPiTaL/mOdEL  ');
    const options = fixture.nativeElement.querySelectorAll('[role="option"]');
    expect(options.length).toBe(1);
    expect(options[0].textContent).toContain('Hospital/Model');
    const selected = vi.fn();
    fixture.componentInstance.selectionChange.subscribe(selected);
    options[0].click();
    await fixture.whenStable();
    expect(selected).toHaveBeenCalledWith(repositories[1]);
    expect(fixture.nativeElement.querySelector('.repository-trigger').textContent).toContain('Hospital/Model');
    expect(fixture.nativeElement.querySelector('.repository-dropdown')).toBeNull();
  });

  it('cancels stale pagination and clears selection and search when switching providers', async () => {
    const fixture = create();
    const http = TestBed.inject(HttpTestingController);
    http.expectOne('/connectors/github/repositories?page=1').flush({ repositories, has_more: true });
    const stale = http.expectOne('/connectors/github/repositories?page=2');
    search(fixture, 'Training');
    fixture.nativeElement.querySelector('[role="option"]').click();
    fixture.detectChanges();
    fixture.componentRef.setInput('provider', 'gitlab');
    fixture.detectChanges();
    expect(stale.cancelled).toBe(true);
    expect(fixture.nativeElement.querySelector('.repository-trigger').textContent).toContain('Select repository');
    fixture.nativeElement.querySelector('.repository-trigger').click();
    fixture.detectChanges();
    http.expectOne('/connectors/gitlab/repositories?page=1').flush({ repositories: [], has_more: false });
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('input').value).toBe('');
    expect(fixture.nativeElement.textContent).toContain('No repositories available.');
  });

  it('supports keyboard selection, Escape, and no-match feedback', async () => {
    const fixture = create('gitlab');
    TestBed.inject(HttpTestingController).expectOne('/connectors/gitlab/repositories?page=1').flush({ repositories, has_more: false });
    await fixture.whenStable();
    search(fixture, 'absent');
    expect(fixture.nativeElement.textContent).toContain('No matching repositories.');
    search(fixture, '');
    const input = fixture.nativeElement.querySelector('input');
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.repository-trigger').textContent).toContain('Research/Training');
    fixture.nativeElement.querySelector('.repository-trigger').click();
    fixture.detectChanges();
    fixture.nativeElement.querySelector('input').dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelector('.repository-dropdown')).toBeNull();
  });

  it('retries repository failures and explains a missing provider connection', async () => {
    const fixture = create();
    const http = TestBed.inject(HttpTestingController);
    http.expectOne('/connectors/github/repositories?page=1').flush('Missing connection', { status: 404, statusText: 'Not found' });
    await fixture.whenStable();
    expect(fixture.nativeElement.textContent).toContain('Add a GitHub token');
    fixture.nativeElement.querySelector('.repository-status button').click();
    http.expectOne('/connectors/github/repositories?page=1').flush({ repositories, has_more: false });
    await fixture.whenStable();
    expect(fixture.nativeElement.querySelectorAll('[role="option"]').length).toBe(2);
  });

  it('returns to login on an expired local session', async () => {
    const navigate = vi.spyOn(TestBed.inject(Router), 'navigateByUrl').mockResolvedValue(true);
    const fixture = create();
    TestBed.inject(HttpTestingController).expectOne('/connectors/github/repositories?page=1').flush('Expired session', { status: 401, statusText: 'Unauthorized' });
    await fixture.whenStable();
    expect(sessionStorage.getItem('authToken')).toBeNull();
    expect(navigate).toHaveBeenCalledWith('/login');
  });
});
